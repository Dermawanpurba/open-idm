//! Downloads one file: probe, multi-segment or single-stream transfer, pause/resume.

use std::collections::BTreeMap;
use std::io::{self, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_RANGE, RANGE};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

const MIB: u64 = 1024 * 1024;
/// An attempt that moved at least this much counts as progress and resets the retry budget.
const RETRY_RESET_BYTES: u64 = 64 * 1024;
const MAX_BACKOFF: Duration = Duration::from_secs(10);

pub struct Job {
    pub id: String,
    pub url: url::Url,
    pub headers: BTreeMap<String, String>,
    /// `<final>.part`; resume state lives in `<final>.part.json`.
    pub part_path: PathBuf,
    pub final_path: PathBuf,
    /// Size reported by the browser; used only when the server does not say.
    pub expected_size: u64,
}

pub type FreeSpaceFn = Arc<dyn Fn(&Path) -> io::Result<u64> + Send + Sync>;

#[derive(Clone)]
pub struct EngineConfig {
    pub segments: usize,
    pub min_segmented: u64,
    pub retries: u32,
    pub retry_base: Duration,
    pub reserve: u64,
    pub free_space: FreeSpaceFn,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            segments: 8,
            min_segmented: 2 * MIB,
            retries: 5,
            retry_base: Duration::from_millis(500),
            reserve: 100 * MIB,
            free_space: Arc::new(|p| fs4::available_space(p)),
        }
    }
}

#[derive(Default)]
struct ProgressInner {
    downloaded: AtomicU64,
    total: AtomicU64,
    can_resume: AtomicBool,
}

/// Live counters shared between a running download and whoever displays it.
#[derive(Clone, Default)]
pub struct Progress(Arc<ProgressInner>);

impl Progress {
    pub fn downloaded(&self) -> u64 {
        self.0.downloaded.load(Ordering::Relaxed)
    }
    pub fn total(&self) -> u64 {
        self.0.total.load(Ordering::Relaxed)
    }
    pub fn can_resume(&self) -> bool {
        self.0.can_resume.load(Ordering::Relaxed)
    }
    fn set_downloaded(&self, n: u64) {
        self.0.downloaded.store(n, Ordering::Relaxed);
    }
    fn add(&self, n: u64) {
        self.0.downloaded.fetch_add(n, Ordering::Relaxed);
    }
    fn set_total(&self, n: u64) {
        self.0.total.store(n, Ordering::Relaxed);
    }
    fn set_can_resume(&self, v: bool) {
        self.0.can_resume.store(v, Ordering::Relaxed);
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Completed { path: PathBuf, size: u64 },
    Paused,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("server rejected the request with HTTP {0}")]
    Rejected(u16),
    #[error("download link expired")]
    Expired,
    #[error("not enough disk space on {drive}: need {need}, free {free}")]
    DiskFull { drive: String, need: u64, free: u64 },
    #[error("download folder unavailable: {0}")]
    DirUnavailable(PathBuf),
    #[error("network error: {0}")]
    Network(String),
    #[error("file error: {0}")]
    Io(String),
}

impl EngineError {
    /// Message safe to show to the user (no internals).
    pub fn user_message(&self) -> String {
        match self {
            EngineError::Rejected(status) => format!("Server menolak unduhan (HTTP {status})."),
            EngineError::Expired => "Tautan sudah kedaluwarsa. Unduh ulang dari website.".into(),
            EngineError::DiskFull { drive, need, free } => {
                let short = need.saturating_sub(*free).div_ceil(MIB).max(1);
                format!("Ruang disk tidak cukup di drive {drive}. Kurang {short} MB.")
            }
            EngineError::DirUnavailable(path) => format!("Folder unduhan tidak bisa diakses: {}", path.display()),
            EngineError::Network(_) => "Koneksi terputus. Coba lanjutkan.".into(),
            EngineError::Io(_) => "File tidak bisa ditulis. Periksa folder unduhan lalu coba lagi.".into(),
        }
    }
}

pub fn build_client() -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("TLS backend initialises")
}

#[derive(Serialize, Deserialize, Clone)]
struct SegState {
    start: u64,
    /// Inclusive.
    end: u64,
    /// Bytes already written for this segment.
    done: u64,
}

#[derive(Serialize, Deserialize)]
struct State {
    url: String,
    total: u64,
    segments: Vec<SegState>,
}

fn state_path(part: &Path) -> PathBuf {
    let mut s = part.as_os_str().to_owned();
    s.push(".json");
    PathBuf::from(s)
}

fn load_state(job: &Job) -> Option<State> {
    if !job.part_path.is_file() {
        return None;
    }
    let state: State = serde_json::from_slice(&std::fs::read(state_path(&job.part_path)).ok()?).ok()?;
    let consistent = state.url == job.url.as_str()
        && !state.segments.is_empty()
        && state.segments.iter().all(|s| s.start <= s.end && s.end < state.total && s.done <= s.end - s.start + 1);
    consistent.then_some(state)
}

fn save_state(part: &Path, state: &State) -> io::Result<()> {
    let target = state_path(part);
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, serde_json::to_vec(state)?)?;
    std::fs::rename(&tmp, target)
}

fn header_map(headers: &BTreeMap<String, String>) -> HeaderMap {
    headers
        .iter()
        .filter_map(|(k, v)| Some((HeaderName::from_bytes(k.as_bytes()).ok()?, HeaderValue::from_str(v).ok()?)))
        .collect()
}

fn drive_of(path: &Path) -> String {
    match path.components().next() {
        Some(Component::Prefix(p)) => p.as_os_str().to_string_lossy().into_owned(),
        _ => path.display().to_string(),
    }
}

fn net(e: reqwest::Error) -> String {
    e.without_url().to_string()
}

fn io_err(e: io::Error) -> EngineError {
    EngineError::Io(e.to_string())
}

fn backoff(base: Duration, attempt: u32) -> Duration {
    base.saturating_mul(1u32 << attempt.saturating_sub(1).min(16)).min(MAX_BACKOFF)
}

/// Sleeps unless stopped first; returns false when stopped.
async fn pause_between(stop: &CancellationToken, wait: Duration) -> bool {
    tokio::select! {
        _ = stop.cancelled() => false,
        _ = tokio::time::sleep(wait) => true,
    }
}

type FirstByte = Mutex<Option<oneshot::Sender<()>>>;

fn signal_first(first: &FirstByte) {
    if let Some(tx) = first.lock().unwrap_or_else(|p| p.into_inner()).take() {
        let _ = tx.send(());
    }
}

struct Probe {
    response: Response,
    ranges: bool,
    total: Option<u64>,
}

async fn probe(client: &Client, job: &Job, headers: &HeaderMap, resuming: bool) -> Result<Probe, EngineError> {
    let response = client
        .get(job.url.clone())
        .headers(headers.clone())
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|e| EngineError::Network(net(e)))?;
    let status = response.status();
    if status == StatusCode::PARTIAL_CONTENT {
        let total = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.parse::<u64>().ok());
        return Ok(Probe { response, ranges: total.is_some(), total });
    }
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        // Empty file, or a server that dislikes the probe: ask again without a range.
        let response = client
            .get(job.url.clone())
            .headers(headers.clone())
            .send()
            .await
            .map_err(|e| EngineError::Network(net(e)))?;
        if !response.status().is_success() {
            return Err(EngineError::Rejected(response.status().as_u16()));
        }
        let total = response.content_length();
        return Ok(Probe { response, ranges: false, total });
    }
    if !status.is_success() {
        let expired = resuming && matches!(status.as_u16(), 403 | 410);
        return Err(if expired { EngineError::Expired } else { EngineError::Rejected(status.as_u16()) });
    }
    let total = response.content_length();
    Ok(Probe { response, ranges: false, total })
}

/// Runs one download to completion, pause, or failure.
///
/// Cancelling `stop` makes it save its position and return `Outcome::Paused`.
/// `first_byte` fires once, when the first byte of file content arrives.
pub async fn run(
    client: &Client,
    job: &Job,
    cfg: &EngineConfig,
    progress: &Progress,
    stop: CancellationToken,
    first_byte: Option<oneshot::Sender<()>>,
) -> Result<Outcome, EngineError> {
    let dir = job.part_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    if tokio::fs::create_dir_all(&dir).await.is_err() || !dir.is_dir() {
        return Err(EngineError::DirUnavailable(dir));
    }
    let headers = header_map(&job.headers);
    let saved = load_state(job);

    let mut attempt = 0;
    let probed = loop {
        match probe(client, job, &headers, saved.is_some()).await {
            Ok(p) => break p,
            Err(EngineError::Network(msg)) => {
                attempt += 1;
                if attempt > cfg.retries {
                    return Err(EngineError::Network(msg));
                }
                if !pause_between(&stop, backoff(cfg.retry_base, attempt)).await {
                    return Ok(Outcome::Paused);
                }
            }
            Err(other) => return Err(other),
        }
    };

    let total = probed.total.filter(|t| *t > 0);
    progress.set_total(total.unwrap_or(job.expected_size));
    progress.set_can_resume(probed.ranges);

    if let Some(total) = total {
        let allocated = std::fs::metadata(&job.part_path).map(|m| m.len()).unwrap_or(0);
        let need = total.saturating_sub(allocated).saturating_add(cfg.reserve);
        let free = (cfg.free_space)(&dir).map_err(|_| EngineError::DirUnavailable(dir.clone()))?;
        if free < need {
            return Err(EngineError::DiskFull { drive: drive_of(&dir), need, free });
        }
    }

    let first = Arc::new(Mutex::new(first_byte));
    let segmented = match total {
        Some(total) if probed.ranges && total >= cfg.min_segmented => Some(total),
        _ => None,
    };

    if let Some(total) = segmented {
        let segments = match saved {
            Some(state) if state.total == total => state.segments,
            _ => fresh_segments(&job.part_path, total, cfg.segments).await?,
        };
        drop(probed.response);
        let ctx = Arc::new(SegCtx {
            client: client.clone(),
            url: job.url.clone(),
            headers: headers.clone(),
            part: job.part_path.clone(),
            done: segments.iter().map(|s| AtomicU64::new(s.done)).collect(),
            ranges: segments.iter().map(|s| (s.start, s.end)).collect(),
            total,
            progress: progress.clone(),
            stop: stop.child_token(),
            first: first.clone(),
            retries: cfg.retries,
            retry_base: cfg.retry_base,
        });
        match run_segments(ctx, &stop).await? {
            SegmentsEnd::Completed => return finish(job, total).await,
            SegmentsEnd::Paused => return Ok(Outcome::Paused),
            SegmentsEnd::RangeIgnored => {
                // The server stopped honouring ranges: start over on one connection.
                let _ = tokio::fs::remove_file(state_path(&job.part_path)).await;
                progress.set_can_resume(false);
                return single_stream(client, job, &headers, cfg, progress, &stop, &first, None, Some(total)).await;
            }
        }
    }

    // A 200 answer to the probe already carries the whole file; a 206 carries one byte.
    let reuse = (probed.response.status() == StatusCode::OK).then_some(probed.response);
    let _ = tokio::fs::remove_file(state_path(&job.part_path)).await;
    single_stream(client, job, &headers, cfg, progress, &stop, &first, reuse, total).await
}

async fn fresh_segments(part: &Path, total: u64, count: usize) -> Result<Vec<SegState>, EngineError> {
    let file = File::create(part).await.map_err(io_err)?;
    file.set_len(total).await.map_err(io_err)?;
    let count = (count.max(1) as u64).min(total);
    let size = total / count;
    Ok((0..count)
        .map(|i| SegState { start: i * size, end: if i == count - 1 { total - 1 } else { (i + 1) * size - 1 }, done: 0 })
        .collect())
}

async fn finish(job: &Job, size: u64) -> Result<Outcome, EngineError> {
    // Antivirus and indexers can hold a fresh file briefly; give the rename a few tries.
    let mut last = None;
    for _ in 0..25 {
        match tokio::fs::rename(&job.part_path, &job.final_path).await {
            Ok(()) => {
                let _ = tokio::fs::remove_file(state_path(&job.part_path)).await;
                return Ok(Outcome::Completed { path: job.final_path.clone(), size });
            }
            Err(e) => last = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(io_err(last.expect("loop ran")))
}

struct SegCtx {
    client: Client,
    url: url::Url,
    headers: HeaderMap,
    part: PathBuf,
    ranges: Vec<(u64, u64)>,
    done: Vec<AtomicU64>,
    total: u64,
    progress: Progress,
    stop: CancellationToken,
    first: Arc<FirstByte>,
    retries: u32,
    retry_base: Duration,
}

impl SegCtx {
    fn snapshot(&self) -> State {
        State {
            url: self.url.to_string(),
            total: self.total,
            segments: self
                .ranges
                .iter()
                .zip(&self.done)
                .map(|(&(start, end), done)| SegState { start, end, done: done.load(Ordering::SeqCst) })
                .collect(),
        }
    }
    fn complete(&self) -> bool {
        self.ranges.iter().zip(&self.done).all(|(&(s, e), d)| d.load(Ordering::SeqCst) == e - s + 1)
    }
    fn downloaded(&self) -> u64 {
        self.done.iter().map(|d| d.load(Ordering::SeqCst)).sum()
    }
}

enum SegmentsEnd {
    Completed,
    Paused,
    RangeIgnored,
}

enum SegEnd {
    Done,
    Stopped,
    RangeIgnored,
}

enum AttemptError {
    Retry(String),
    Fatal(EngineError),
}

async fn run_segments(ctx: Arc<SegCtx>, stop: &CancellationToken) -> Result<SegmentsEnd, EngineError> {
    ctx.progress.set_downloaded(ctx.downloaded());
    let mut set = JoinSet::new();
    for idx in 0..ctx.ranges.len() {
        set.spawn(segment(ctx.clone(), idx));
    }
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    let mut failure = None;
    let mut range_ignored = false;
    loop {
        tokio::select! {
            _ = ticker.tick() => { let _ = save_state(&ctx.part, &ctx.snapshot()); }
            joined = set.join_next() => match joined {
                None => break,
                Some(Ok(Ok(SegEnd::Done | SegEnd::Stopped))) => {}
                Some(Ok(Ok(SegEnd::RangeIgnored))) => { range_ignored = true; ctx.stop.cancel(); }
                Some(Ok(Err(e))) => { failure.get_or_insert(e); ctx.stop.cancel(); }
                Some(Err(e)) => { failure.get_or_insert(EngineError::Io(e.to_string())); ctx.stop.cancel(); }
            }
        }
    }
    save_state(&ctx.part, &ctx.snapshot()).map_err(io_err)?;
    if range_ignored {
        return Ok(SegmentsEnd::RangeIgnored);
    }
    if let Some(e) = failure {
        return Err(e);
    }
    if ctx.complete() {
        return Ok(SegmentsEnd::Completed);
    }
    debug_assert!(stop.is_cancelled());
    Ok(SegmentsEnd::Paused)
}

async fn segment(ctx: Arc<SegCtx>, idx: usize) -> Result<SegEnd, EngineError> {
    let (start, end) = ctx.ranges[idx];
    let len = end - start + 1;
    let mut attempts = 0;
    loop {
        let before = ctx.done[idx].load(Ordering::SeqCst);
        if before >= len {
            return Ok(SegEnd::Done);
        }
        let result = tokio::select! {
            _ = ctx.stop.cancelled() => return Ok(SegEnd::Stopped),
            r = segment_attempt(&ctx, idx) => r,
        };
        match result {
            Ok(end) => return Ok(end),
            Err(AttemptError::Fatal(e)) => return Err(e),
            Err(AttemptError::Retry(msg)) => {
                if ctx.done[idx].load(Ordering::SeqCst) - before >= RETRY_RESET_BYTES {
                    attempts = 0;
                }
                attempts += 1;
                if attempts > ctx.retries {
                    return Err(EngineError::Network(msg));
                }
                if !pause_between(&ctx.stop, backoff(ctx.retry_base, attempts)).await {
                    return Ok(SegEnd::Stopped);
                }
            }
        }
    }
}

async fn segment_attempt(ctx: &SegCtx, idx: usize) -> Result<SegEnd, AttemptError> {
    let (start, end) = ctx.ranges[idx];
    let len = end - start + 1;
    let done = ctx.done[idx].load(Ordering::SeqCst);
    let response = ctx
        .client
        .get(ctx.url.clone())
        .headers(ctx.headers.clone())
        .header(RANGE, format!("bytes={}-{}", start + done, end))
        .send()
        .await
        .map_err(|e| AttemptError::Retry(net(e)))?;
    match response.status().as_u16() {
        206 => {}
        200 => return Ok(SegEnd::RangeIgnored),
        403 | 410 => return Err(AttemptError::Fatal(EngineError::Expired)),
        other => return Err(AttemptError::Retry(format!("HTTP {other}"))),
    }
    let fatal = |e: io::Error| AttemptError::Fatal(io_err(e));
    let mut file = OpenOptions::new().write(true).open(&ctx.part).await.map_err(fatal)?;
    file.seek(SeekFrom::Start(start + done)).await.map_err(fatal)?;
    let mut stream = response.bytes_stream();
    let mut written = done;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AttemptError::Retry(net(e)))?;
        let take = (chunk.len() as u64).min(len - written) as usize;
        file.write_all(&chunk[..take]).await.map_err(fatal)?;
        // Flush before counting: a counted byte must really be in the file when state is saved.
        file.flush().await.map_err(fatal)?;
        written += take as u64;
        ctx.done[idx].store(written, Ordering::SeqCst);
        ctx.progress.add(take as u64);
        signal_first(&ctx.first);
        if written >= len {
            return Ok(SegEnd::Done);
        }
    }
    Err(AttemptError::Retry("connection closed early".into()))
}

#[allow(clippy::too_many_arguments)]
async fn single_stream(
    client: &Client,
    job: &Job,
    headers: &HeaderMap,
    cfg: &EngineConfig,
    progress: &Progress,
    stop: &CancellationToken,
    first: &FirstByte,
    mut reuse: Option<Response>,
    total: Option<u64>,
) -> Result<Outcome, EngineError> {
    let mut attempts = 0;
    loop {
        let result = tokio::select! {
            _ = stop.cancelled() => return Ok(Outcome::Paused),
            r = single_attempt(client, job, headers, progress, first, reuse.take(), total) => r,
        };
        match result {
            Ok(size) => return finish(job, size).await,
            Err(AttemptError::Fatal(e)) => return Err(e),
            Err(AttemptError::Retry(msg)) => {
                attempts += 1;
                if attempts > cfg.retries {
                    return Err(EngineError::Network(msg));
                }
                if !pause_between(stop, backoff(cfg.retry_base, attempts)).await {
                    return Ok(Outcome::Paused);
                }
            }
        }
    }
}

async fn single_attempt(
    client: &Client,
    job: &Job,
    headers: &HeaderMap,
    progress: &Progress,
    first: &FirstByte,
    reuse: Option<Response>,
    total: Option<u64>,
) -> Result<u64, AttemptError> {
    let response = match reuse {
        Some(r) => r,
        None => {
            let r = client
                .get(job.url.clone())
                .headers(headers.clone())
                .send()
                .await
                .map_err(|e| AttemptError::Retry(net(e)))?;
            match r.status().as_u16() {
                200..=299 => r,
                s @ (500..=599 | 429) => return Err(AttemptError::Retry(format!("HTTP {s}"))),
                s => return Err(AttemptError::Fatal(EngineError::Rejected(s))),
            }
        }
    };
    let fatal = |e: io::Error| AttemptError::Fatal(io_err(e));
    let mut file = File::create(&job.part_path).await.map_err(fatal)?;
    progress.set_downloaded(0);
    let mut written = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AttemptError::Retry(net(e)))?;
        file.write_all(&chunk).await.map_err(fatal)?;
        written += chunk.len() as u64;
        progress.add(chunk.len() as u64);
        if !chunk.is_empty() {
            signal_first(first);
        }
    }
    file.flush().await.map_err(fatal)?;
    drop(file);
    match total {
        Some(expected) if written != expected => Err(AttemptError::Retry("incomplete response".into())),
        _ => Ok(written),
    }
}
