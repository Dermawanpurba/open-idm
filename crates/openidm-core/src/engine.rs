//! Downloads one file: probe, multi-segment or single-stream transfer, pause/resume.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_RANGE, RANGE, RETRY_AFTER};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

const MIB: u64 = 1024 * 1024;
/// An attempt that moved at least this much counts as progress and resets the retry budget.
const RETRY_RESET_BYTES: u64 = 64 * 1024;
const MAX_BACKOFF: Duration = Duration::from_secs(10);
/// A range is split only when both halves would be at least this big.
const MIN_SPLIT: u64 = 256 * 1024;
/// Buffered bytes per connection before they are written to disk.
const FLUSH_BYTES: usize = 512 * 1024;
/// Written at least this often so progress and resume state stay current on slow links.
const FLUSH_EVERY: Duration = Duration::from_millis(500);
/// Sent when the download did not come with the browser's own User-Agent.
pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

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
    segments: Mutex<Vec<SegmentView>>,
}

/// One range of a multi-connection download, as shown in the UI.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SegmentView {
    pub start: u64,
    /// Inclusive.
    pub end: u64,
    pub done: u64,
    /// A connection is working on it right now.
    pub active: bool,
    /// Bytes per second over the last second.
    pub speed: f64,
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
    /// Ranges of a multi-connection download, sorted by start. Empty for single-connection downloads.
    pub fn segments(&self) -> Vec<SegmentView> {
        self.0.segments.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    pub(crate) fn set_segments(&self, segments: Vec<SegmentView>) {
        *self.0.segments.lock().unwrap_or_else(|p| p.into_inner()) = segments;
    }
    pub(crate) fn set_downloaded(&self, n: u64) {
        self.0.downloaded.store(n, Ordering::Relaxed);
    }
    fn add(&self, n: u64) {
        self.0.downloaded.fetch_add(n, Ordering::Relaxed);
    }
    pub(crate) fn set_total(&self, n: u64) {
        self.0.total.store(n, Ordering::Relaxed);
    }
    pub(crate) fn set_can_resume(&self, v: bool) {
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
    #[error("{0}")]
    Video(String),
    #[error("server rejected the request with HTTP {0}")]
    Rejected(u16),
    #[error("download link expired")]
    Expired,
    #[error("redirected to a sign-in page")]
    LoginRequired,
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
            EngineError::Video(message) => message.clone(),
            EngineError::Rejected(status) => format!("Server menolak unduhan (HTTP {status})."),
            EngineError::Expired => "Tautan sudah kedaluwarsa. Unduh ulang dari website.".into(),
            EngineError::LoginRequired => {
                "Situs meminta login. Unduh ulang lewat tombol unduh di browser agar sesi login ikut terbawa.".into()
            }
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
        .user_agent(DEFAULT_USER_AGENT)
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
    let mut ranges: Vec<&SegState> = state.segments.iter().collect();
    ranges.sort_by_key(|s| s.start);
    // The ranges must tile the file exactly: no gaps, no overlaps.
    let tiles = ranges.first().is_some_and(|s| s.start == 0)
        && ranges.last().is_some_and(|s| s.end + 1 == state.total)
        && ranges.windows(2).all(|w| w[0].end + 1 == w[1].start);
    let consistent = state.url == job.url.as_str()
        && tiles
        && state.segments.iter().all(|s| s.start <= s.end && s.done <= s.end - s.start + 1);
    consistent.then_some(state)
}

/// Segments recorded for a paused or failed multi-connection download (empty when there are none).
pub fn saved_segments(part: &Path) -> Vec<SegmentView> {
    let Ok(bytes) = std::fs::read(state_path(part)) else { return Vec::new() };
    let Ok(state) = serde_json::from_slice::<State>(&bytes) else { return Vec::new() };
    let mut views: Vec<SegmentView> = state
        .segments
        .iter()
        .filter(|s| s.start <= s.end)
        .map(|s| SegmentView { start: s.start, end: s.end, done: s.done.min(s.end - s.start + 1), active: false, speed: 0.0 })
        .collect();
    views.sort_by_key(|v| v.start);
    views
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

/// True when a redirect left the requested URL for a sign-in page.
fn is_sign_in_redirect(requested: &url::Url, landed: &url::Url) -> bool {
    if requested == landed {
        return false;
    }
    let host = landed.host_str().unwrap_or("").to_ascii_lowercase();
    let path = landed.path().to_ascii_lowercase();
    host.starts_with("accounts.")
        || host.starts_with("login.")
        || ["servicelogin", "/signin", "/login", "/sign-in", "/auth/login"].iter().any(|marker| path.contains(marker))
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
    // Sites that need a session (Google Drive, Takeout) redirect cookieless requests to sign in.
    // Whatever that page answers, it is not the file.
    if is_sign_in_redirect(&job.url, response.url()) {
        return Err(EngineError::LoginRequired);
    }
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
            Err(retryable @ (EngineError::Network(_) | EngineError::Rejected(429 | 503))) => {
                attempt += 1;
                if attempt > cfg.retries {
                    return Err(retryable);
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
        let file = std::fs::OpenOptions::new().write(true).open(&job.part_path).map_err(io_err)?;
        let connections = cfg.segments.max(1);
        let ctx = Arc::new(SegCtx {
            client: client.clone(),
            url: job.url.clone(),
            headers: headers.clone(),
            part: job.part_path.clone(),
            file: Arc::new(file),
            plan: Mutex::new(Plan {
                segs: segments
                    .iter()
                    .map(|s| Seg { start: s.start, end: s.end, done: s.done, reserved: 0, active: false, speed: 0.0, last_done: s.done })
                    .collect(),
                alive: connections,
            }),
            total,
            progress: progress.clone(),
            stop: stop.child_token(),
            first: first.clone(),
            retries: cfg.retries,
            retry_base: cfg.retry_base,
            range_ignored: AtomicBool::new(false),
        });
        let outcome = run_segments(ctx.clone(), connections, &stop).await;
        drop(ctx);
        match outcome? {
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
    // Very small files get fewer segments: each must be worth a connection.
    let count = (count.max(1) as u64).min((total / MIN_SPLIT).max(1));
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

/// One byte range of the file. A connection owns it while `active`.
struct Seg {
    start: u64,
    /// Inclusive. Shrinks when another connection splits this segment.
    end: u64,
    /// Bytes written to disk from `start`.
    done: u64,
    /// Bytes received and buffered but not yet written. A split never cuts into them.
    reserved: u64,
    active: bool,
    speed: f64,
    last_done: u64,
}

impl Seg {
    fn len(&self) -> u64 {
        self.end - self.start + 1
    }
    fn remaining(&self) -> u64 {
        self.len().saturating_sub(self.done + self.reserved)
    }
    fn complete(&self) -> bool {
        self.done >= self.len()
    }
}

/// The shared work plan: which ranges exist and how many connections are still working.
struct Plan {
    segs: Vec<Seg>,
    /// Connections that have not exited yet.
    alive: usize,
}

impl Plan {
    /// Gives the calling connection a range: an idle unfinished segment, or the second half of the
    /// largest range another connection is still working on. `None` means the connection exits.
    fn claim(&mut self) -> Option<usize> {
        if let Some(idx) = self.segs.iter().position(|s| !s.active && !s.complete()) {
            self.segs[idx].active = true;
            return Some(idx);
        }
        let victim = (0..self.segs.len())
            .filter(|&i| self.segs[i].active && self.segs[i].remaining() >= 2 * MIN_SPLIT)
            .max_by_key(|&i| self.segs[i].remaining());
        if let Some(v) = victim {
            let seg = &mut self.segs[v];
            let cut = seg.start + seg.done + seg.reserved + seg.remaining() / 2;
            let tail = Seg { start: cut, end: seg.end, done: 0, reserved: 0, active: true, speed: 0.0, last_done: 0 };
            seg.end = cut - 1;
            self.segs.push(tail);
            return Some(self.segs.len() - 1);
        }
        self.alive -= 1;
        None
    }

    /// A throttled or failing connection gives its range back, unless it is the last one working.
    fn try_retire(&mut self, idx: usize) -> bool {
        if self.alive <= 1 {
            return false;
        }
        self.segs[idx].active = false;
        self.alive -= 1;
        true
    }

    fn views(&self) -> Vec<SegmentView> {
        let mut views: Vec<SegmentView> = self
            .segs
            .iter()
            .map(|s| SegmentView { start: s.start, end: s.end, done: s.done.min(s.len()), active: s.active, speed: s.speed })
            .collect();
        views.sort_by_key(|v| v.start);
        views
    }
}

struct SegCtx {
    client: Client,
    url: url::Url,
    headers: HeaderMap,
    part: PathBuf,
    file: Arc<std::fs::File>,
    plan: Mutex<Plan>,
    total: u64,
    progress: Progress,
    stop: CancellationToken,
    first: Arc<FirstByte>,
    retries: u32,
    retry_base: Duration,
    range_ignored: AtomicBool,
}

impl SegCtx {
    fn plan(&self) -> std::sync::MutexGuard<'_, Plan> {
        self.plan.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn snapshot(&self) -> State {
        let plan = self.plan();
        let mut segments: Vec<SegState> =
            plan.segs.iter().map(|s| SegState { start: s.start, end: s.end, done: s.done.min(s.len()) }).collect();
        segments.sort_by_key(|s| s.start);
        State { url: self.url.to_string(), total: self.total, segments }
    }
    fn complete(&self) -> bool {
        self.plan().segs.iter().all(Seg::complete)
    }
    fn downloaded(&self) -> u64 {
        self.plan().segs.iter().map(|s| s.done.min(s.len())).sum()
    }
    /// Updates per-segment speeds (bytes/s over `elapsed`) and publishes the view.
    fn tick(&self, elapsed: Duration) {
        let mut plan = self.plan();
        let secs = elapsed.as_secs_f64().max(0.001);
        for s in plan.segs.iter_mut() {
            s.speed = if s.active { s.done.saturating_sub(s.last_done) as f64 / secs } else { 0.0 };
            s.last_done = s.done;
        }
        self.progress.set_segments(plan.views());
    }
}

enum SegmentsEnd {
    Completed,
    Paused,
    RangeIgnored,
}

async fn run_segments(ctx: Arc<SegCtx>, connections: usize, stop: &CancellationToken) -> Result<SegmentsEnd, EngineError> {
    ctx.progress.set_downloaded(ctx.downloaded());
    let mut set = JoinSet::new();
    for _ in 0..connections {
        set.spawn(connection(ctx.clone()));
    }
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    let mut last_tick = tokio::time::Instant::now();
    let mut failure = None;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                ctx.tick(last_tick.elapsed());
                last_tick = tokio::time::Instant::now();
                let _ = save_state(&ctx.part, &ctx.snapshot());
            }
            joined = set.join_next() => match joined {
                None => break,
                Some(Ok(Ok(()))) => {}
                Some(Ok(Err(e))) => { failure.get_or_insert(e); ctx.stop.cancel(); }
                Some(Err(e)) => { failure.get_or_insert(EngineError::Io(e.to_string())); ctx.stop.cancel(); }
            }
        }
    }
    {
        let mut plan = ctx.plan();
        for s in plan.segs.iter_mut() {
            s.active = false;
            s.speed = 0.0;
        }
        ctx.progress.set_segments(plan.views());
    }
    save_state(&ctx.part, &ctx.snapshot()).map_err(io_err)?;
    if ctx.range_ignored.load(Ordering::SeqCst) {
        return Ok(SegmentsEnd::RangeIgnored);
    }
    if let Some(e) = failure {
        return Err(e);
    }
    if ctx.complete() {
        return Ok(SegmentsEnd::Completed);
    }
    if stop.is_cancelled() {
        return Ok(SegmentsEnd::Paused);
    }
    Err(EngineError::Network("connections ended before the file was complete".into()))
}

enum SegEnd {
    /// The range is finished; take another.
    Done,
    /// Pause requested.
    Stopped,
    /// This connection gave its range back to the others.
    Retired,
}

enum AttemptError {
    Retry(String),
    /// The server is limiting connections (429/503).
    Throttled(u16, Option<Duration>),
    RangeIgnored,
    Fatal(EngineError),
}

/// One connection: keeps taking ranges until there is nothing left, it is paused, or it retires.
async fn connection(ctx: Arc<SegCtx>) -> Result<(), EngineError> {
    loop {
        if ctx.stop.is_cancelled() {
            ctx.plan().alive -= 1;
            return Ok(());
        }
        let Some(idx) = ctx.plan().claim() else { return Ok(()) };
        match transfer(&ctx, idx).await {
            Ok(SegEnd::Done) => ctx.plan().segs[idx].active = false,
            Ok(SegEnd::Retired) => return Ok(()),
            Ok(SegEnd::Stopped) => {
                let mut plan = ctx.plan();
                plan.segs[idx].active = false;
                plan.alive -= 1;
                return Ok(());
            }
            Err(e) => {
                let mut plan = ctx.plan();
                plan.segs[idx].active = false;
                plan.alive -= 1;
                return Err(e);
            }
        }
    }
}

/// Downloads range `idx` with retries. Gives up on the range (retires) when the server throttles or the
/// connection keeps failing, as long as another connection is still working.
async fn transfer(ctx: &SegCtx, idx: usize) -> Result<SegEnd, EngineError> {
    let mut attempts = 0;
    loop {
        let before = ctx.plan().segs[idx].done;
        let result = attempt(ctx, idx).await;
        let wait = match result {
            Ok(end) => return Ok(end),
            Err(AttemptError::Fatal(e)) => return Err(e),
            Err(AttemptError::RangeIgnored) => {
                ctx.range_ignored.store(true, Ordering::SeqCst);
                ctx.stop.cancel();
                return Ok(SegEnd::Stopped);
            }
            Err(AttemptError::Throttled(status, retry_after)) => {
                // Too many connections for this server: hand the range to the others and stop.
                if ctx.plan().try_retire(idx) {
                    return Ok(SegEnd::Retired);
                }
                attempts += 1;
                if attempts > ctx.retries {
                    return Err(EngineError::Rejected(status));
                }
                retry_after.unwrap_or_else(|| backoff(ctx.retry_base, attempts)).min(MAX_BACKOFF)
            }
            Err(AttemptError::Retry(msg)) => {
                if ctx.plan().segs[idx].done.saturating_sub(before) >= RETRY_RESET_BYTES {
                    attempts = 0;
                }
                attempts += 1;
                if attempts > ctx.retries {
                    if ctx.plan().try_retire(idx) {
                        return Ok(SegEnd::Retired);
                    }
                    return Err(EngineError::Network(msg));
                }
                backoff(ctx.retry_base, attempts)
            }
        };
        if !pause_between(&ctx.stop, wait).await {
            return Ok(SegEnd::Stopped);
        }
    }
}

fn retry_after(response: &Response) -> Option<Duration> {
    let secs = response.headers().get(RETRY_AFTER)?.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(secs))
}

/// Writes `data` at `offset` without moving a shared cursor, off the async threads.
async fn write_at(file: &Arc<std::fs::File>, offset: u64, data: Vec<u8>) -> io::Result<()> {
    let file = file.clone();
    tokio::task::spawn_blocking(move || {
        let mut written = 0;
        while written < data.len() {
            #[cfg(windows)]
            let n = std::os::windows::fs::FileExt::seek_write(&*file, &data[written..], offset + written as u64)?;
            #[cfg(unix)]
            let n = std::os::unix::fs::FileExt::write_at(&*file, &data[written..], offset + written as u64)?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            written += n;
        }
        Ok(())
    })
    .await
    .map_err(|e| io::Error::other(e.to_string()))?
}

/// Writes the buffered bytes of range `idx` and counts them as done.
async fn flush(ctx: &SegCtx, idx: usize, buffer: &mut Vec<u8>) -> Result<(), AttemptError> {
    if buffer.is_empty() {
        return Ok(());
    }
    let offset = {
        let plan = ctx.plan();
        plan.segs[idx].start + plan.segs[idx].done
    };
    let data = std::mem::take(buffer);
    let len = data.len() as u64;
    write_at(&ctx.file, offset, data).await.map_err(|e| AttemptError::Fatal(io_err(e)))?;
    {
        let mut plan = ctx.plan();
        let seg = &mut plan.segs[idx];
        seg.done += len;
        seg.reserved -= len;
    }
    ctx.progress.add(len);
    Ok(())
}

/// One HTTP request for the rest of range `idx`.
async fn attempt(ctx: &SegCtx, idx: usize) -> Result<SegEnd, AttemptError> {
    let (from, to) = {
        let plan = ctx.plan();
        let s = &plan.segs[idx];
        if s.complete() {
            return Ok(SegEnd::Done);
        }
        (s.start + s.done, s.end)
    };
    let request = ctx.client.get(ctx.url.clone()).headers(ctx.headers.clone()).header(RANGE, format!("bytes={from}-{to}"));
    let response = tokio::select! {
        _ = ctx.stop.cancelled() => return Ok(SegEnd::Stopped),
        r = request.send() => r.map_err(|e| AttemptError::Retry(net(e)))?,
    };
    match response.status().as_u16() {
        206 => {}
        200 => return Err(AttemptError::RangeIgnored),
        403 | 410 => return Err(AttemptError::Fatal(EngineError::Expired)),
        status @ (429 | 503) => return Err(AttemptError::Throttled(status, retry_after(&response))),
        other => return Err(AttemptError::Retry(format!("HTTP {other}"))),
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::with_capacity(FLUSH_BYTES);
    let mut last_flush = tokio::time::Instant::now();
    loop {
        let chunk = tokio::select! {
            _ = ctx.stop.cancelled() => {
                flush(ctx, idx, &mut buffer).await?;
                return Ok(SegEnd::Stopped);
            }
            chunk = stream.next() => chunk,
        };
        let chunk = match chunk {
            Some(Ok(chunk)) => chunk,
            Some(Err(e)) => {
                // Keep what already arrived: it is valid data.
                flush(ctx, idx, &mut buffer).await?;
                return Err(AttemptError::Retry(net(e)));
            }
            None => {
                flush(ctx, idx, &mut buffer).await?;
                return if ctx.plan().segs[idx].complete() {
                    Ok(SegEnd::Done)
                } else {
                    Err(AttemptError::Retry("connection closed early".into()))
                };
            }
        };
        // Take only what still belongs to this range: another connection may have split it.
        let (take, reached_end) = {
            let mut plan = ctx.plan();
            let seg = &mut plan.segs[idx];
            let take = (chunk.len() as u64).min(seg.remaining());
            seg.reserved += take;
            (take as usize, seg.remaining() == 0)
        };
        buffer.extend_from_slice(&chunk[..take]);
        if take > 0 {
            signal_first(&ctx.first);
        }
        if reached_end || buffer.len() >= FLUSH_BYTES || last_flush.elapsed() >= FLUSH_EVERY {
            flush(ctx, idx, &mut buffer).await?;
            last_flush = tokio::time::Instant::now();
        }
        if reached_end {
            // The rest of this response belongs to another range; dropping it closes the connection.
            return Ok(SegEnd::Done);
        }
    }
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
            // single_attempt only reports Retry or Fatal; the other kinds belong to ranged transfers.
            Err(AttemptError::Throttled(..) | AttemptError::RangeIgnored) => {
                return Err(EngineError::Network("unexpected response".into()))
            }
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
