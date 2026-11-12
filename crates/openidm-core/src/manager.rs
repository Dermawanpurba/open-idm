//! Owns the download queue: creates tasks, runs at most N at once, tracks live progress.

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::db::{now, Db, TaskRow};
use crate::engine::{self, EngineConfig, Job, Outcome, Progress};
use crate::naming::{category_for, http_url, is_inside, resolve_filename, unique_path};
use crate::protocol::filter_headers;
use crate::settings::Settings;

const SPEED_WINDOW: Duration = Duration::from_secs(5);

#[derive(Default, Clone, Debug)]
pub struct NewTask {
    pub url: String,
    pub filename: Option<String>,
    pub filesize: u64,
    pub headers: BTreeMap<String, String>,
    pub request_id: Option<String>,
    /// Store the task without starting it.
    pub start_paused: bool,
    /// Start right away even when the queue is full (used for downloads taken over from the browser,
    /// which must be confirmed or handed back quickly).
    pub immediate: bool,
}

#[derive(Clone, Debug)]
enum First {
    Waiting,
    Got,
    Failed(String),
}

struct Active {
    stop: CancellationToken,
    progress: Progress,
    /// f64 bits, bytes per second.
    speed: Arc<AtomicU64>,
    /// `Some(delete_file)` once the user asked to delete this running task.
    delete: Arc<Mutex<Option<bool>>>,
}

pub struct Manager {
    data_dir: PathBuf,
    db: Db,
    settings: Mutex<Settings>,
    cfg: EngineConfig,
    client: reqwest::Client,
    rt: Handle,
    active: Mutex<HashMap<String, Active>>,
    first: Mutex<HashMap<String, Arc<watch::Sender<First>>>>,
    scheduling: Mutex<()>,
    shutting_down: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn new_id() -> String {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(now().to_bits());
    format!("{:08x}", hasher.finish() as u32)
}

fn part_of(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn remove_partials(final_path: &Path) {
    let part = part_of(final_path);
    for suffix in ["", ".json", ".json.tmp"] {
        let mut p = part.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(p);
    }
}

impl Manager {
    /// Must be called from within a Tokio runtime; downloads run on that runtime.
    pub fn open(data_dir: &Path) -> Result<Arc<Manager>, String> {
        Self::with_engine_config(data_dir, EngineConfig::default())
    }

    pub fn with_engine_config(data_dir: &Path, cfg: EngineConfig) -> Result<Arc<Manager>, String> {
        let rt = Handle::try_current().map_err(|_| "Mesin unduh belum siap.".to_string())?;
        let db = Db::open(&data_dir.join("downloads.db"))
            .map_err(|_| "Database unduhan tidak bisa dibuka.".to_string())?;
        db.reset_running_to_paused().map_err(|_| "Database unduhan tidak bisa dibaca.".to_string())?;
        Ok(Arc::new(Manager {
            data_dir: data_dir.to_path_buf(),
            db,
            settings: Mutex::new(Settings::load(data_dir)),
            cfg,
            client: engine::build_client(),
            rt,
            active: Mutex::new(HashMap::new()),
            first: Mutex::new(HashMap::new()),
            scheduling: Mutex::new(()),
            shutting_down: AtomicBool::new(false),
        }))
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn update_settings(self: &Arc<Self>, settings: Settings) -> Result<Settings, String> {
        let settings = settings.validated()?;
        settings.save(&self.data_dir).map_err(|_| "Pengaturan tidak bisa disimpan.".to_string())?;
        *lock(&self.settings) = settings.clone();
        self.schedule();
        Ok(settings)
    }

    pub fn add(self: &Arc<Self>, task: NewTask) -> Result<TaskRow, String> {
        let url = http_url(&task.url).ok_or("Tautan tidak valid.")?;
        let filename = resolve_filename(task.filename.as_deref(), None, &url);
        let category = category_for(&filename);
        let root = self.settings().download_dir;
        let dir = root.join(category);
        let _ = std::fs::create_dir_all(&dir);
        let path = unique_path(&dir, &filename);
        if !is_inside(&root, &path) {
            return Err("Nama file tidak valid.".into());
        }
        // Reserve the name so a second task queued before this one starts gets a different file.
        let _ = std::fs::File::create(part_of(&path));

        let filename = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(filename);
        let row = TaskRow {
            id: new_id(),
            url: url.to_string(),
            title: filename.clone(),
            filename,
            filepath: path.to_string_lossy().into_owned(),
            filesize: task.filesize,
            downloaded_bytes: 0,
            progress: 0.0,
            speed: 0.0,
            eta: 0,
            status: if task.start_paused { "paused" } else { "pending" }.into(),
            category: category.into(),
            thumbnail: String::new(),
            format_id: "direct".into(),
            error_message: String::new(),
            request_id: task.request_id.unwrap_or_default(),
            created_at: now(),
            completed_at: None,
            headers: filter_headers(&task.headers),
            can_resume: true,
        };
        self.db.insert(&row).map_err(|_| "Unduhan tidak bisa disimpan.".to_string())?;
        lock(&self.first).insert(row.id.clone(), Arc::new(watch::channel(First::Waiting).0));
        if !task.start_paused {
            if task.immediate {
                self.start(row.clone());
            } else {
                self.schedule();
            }
        }
        Ok(self.get(&row.id).unwrap_or(row))
    }

    /// Resolves once file content starts arriving; fails with a user-facing message otherwise.
    pub async fn wait_first_byte(&self, id: &str) -> Result<(), String> {
        let mut rx = lock(&self.first).get(id).ok_or("Unduhan tidak ditemukan.")?.subscribe();
        loop {
            match &*rx.borrow_and_update() {
                First::Got => return Ok(()),
                First::Failed(message) => return Err(message.clone()),
                First::Waiting => {}
            }
            if rx.changed().await.is_err() {
                return Err("Unduhan dibatalkan.".into());
            }
        }
    }

    pub fn pause(&self, id: &str) -> bool {
        if let Some(active) = lock(&self.active).get(id) {
            active.stop.cancel();
            return true;
        }
        match self.db.get(id) {
            Ok(Some(row)) if row.status == "pending" => self.db.set_status(id, "paused", "").is_ok(),
            _ => false,
        }
    }

    pub fn resume(self: &Arc<Self>, id: &str) -> bool {
        if lock(&self.active).contains_key(id) {
            return true;
        }
        match self.db.get(id) {
            Ok(Some(row)) if matches!(row.status.as_str(), "paused" | "error") => {
                lock(&self.first)
                    .entry(id.to_string())
                    .or_insert_with(|| Arc::new(watch::channel(First::Waiting).0))
                    .send_replace(First::Waiting);
                let queued = self.db.set_status(id, "pending", "").is_ok();
                self.schedule();
                queued
            }
            _ => false,
        }
    }

    /// Removes the task and its partial files; the finished file only when `delete_file`.
    pub fn delete(&self, id: &str, delete_file: bool) -> bool {
        if let Some(active) = lock(&self.active).get(id) {
            *lock(&active.delete) = Some(delete_file);
            active.stop.cancel();
            return true;
        }
        let Ok(Some(row)) = self.db.get(id) else { return false };
        self.remove_task(&row, delete_file);
        true
    }

    fn remove_task(&self, row: &TaskRow, delete_file: bool) {
        let path = Path::new(&row.filepath);
        remove_partials(path);
        if delete_file {
            let _ = std::fs::remove_file(path);
        }
        let _ = self.db.delete(&row.id);
        if let Some(first) = lock(&self.first).remove(&row.id) {
            first.send_replace(First::Failed("Unduhan dihapus.".into()));
        }
    }

    pub fn pause_all(&self) {
        for row in self.db.list(None, Some("pending")).unwrap_or_default() {
            let _ = self.db.set_status(&row.id, "paused", "");
        }
        for active in lock(&self.active).values() {
            active.stop.cancel();
        }
    }

    pub fn resume_all(self: &Arc<Self>) {
        for row in self.db.list(None, Some("paused")).unwrap_or_default() {
            self.resume(&row.id);
        }
    }

    /// Stops every running download, saving its position. Used when the app exits.
    pub fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        for active in lock(&self.active).values() {
            active.stop.cancel();
        }
    }

    pub fn active_count(&self) -> usize {
        lock(&self.active).len()
    }

    fn overlay(&self, mut row: TaskRow) -> TaskRow {
        if let Some(active) = lock(&self.active).get(&row.id) {
            let total = active.progress.total();
            row.downloaded_bytes = active.progress.downloaded();
            if total > 0 {
                row.filesize = total;
                row.progress = (row.downloaded_bytes as f64 / total as f64 * 100.0).min(100.0);
                row.can_resume = active.progress.can_resume();
            }
            row.speed = f64::from_bits(active.speed.load(Ordering::Relaxed));
            row.eta = eta(row.filesize, row.downloaded_bytes, row.speed);
            row.status = "downloading".into();
        }
        row
    }

    pub fn list(&self, category: Option<&str>, status: Option<&str>) -> Vec<TaskRow> {
        self.db.list(category, status).unwrap_or_default().into_iter().map(|r| self.overlay(r)).collect()
    }

    pub fn get(&self, id: &str) -> Option<TaskRow> {
        self.db.get(id).ok().flatten().map(|r| self.overlay(r))
    }

    /// Starts queued tasks, oldest first, while there is room.
    fn schedule(self: &Arc<Self>) {
        let _guard = lock(&self.scheduling);
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        let limit = self.settings().max_concurrent;
        while self.active_count() < limit {
            let pending = self.db.list(None, Some("pending")).unwrap_or_default();
            let next = pending.into_iter().rev().find(|r| !lock(&self.active).contains_key(&r.id));
            match next {
                Some(row) => self.start(row),
                None => break,
            }
        }
    }

    fn start(self: &Arc<Self>, row: TaskRow) {
        let active = Active {
            stop: CancellationToken::new(),
            progress: Progress::default(),
            speed: Arc::new(AtomicU64::new(0)),
            delete: Arc::new(Mutex::new(None)),
        };
        let run = RunCtx {
            stop: active.stop.clone(),
            progress: active.progress.clone(),
            speed: active.speed.clone(),
            delete: active.delete.clone(),
            first: lock(&self.first)
                .entry(row.id.clone())
                .or_insert_with(|| Arc::new(watch::channel(First::Waiting).0))
                .clone(),
        };
        lock(&self.active).insert(row.id.clone(), active);
        let _ = self.db.set_status(&row.id, "downloading", "");
        let manager = self.clone();
        self.rt.spawn(async move { manager.run_task(row, run).await });
    }

    async fn run_task(self: Arc<Self>, row: TaskRow, run: RunCtx) {
        let final_path = PathBuf::from(&row.filepath);
        let job = Job {
            id: row.id.clone(),
            url: match http_url(&row.url) {
                Some(url) => url,
                None => {
                    let _ = self.db.set_status(&row.id, "error", "Tautan tidak valid.");
                    lock(&self.active).remove(&row.id);
                    run.first.send_replace(First::Failed("Tautan tidak valid.".into()));
                    self.schedule();
                    return;
                }
            },
            headers: row.headers.clone(),
            part_path: part_of(&final_path),
            final_path: final_path.clone(),
            expected_size: row.filesize,
        };
        let cfg = EngineConfig { segments: self.settings().segments, ..self.cfg.clone() };
        let (first_tx, mut first_rx) = oneshot::channel();
        let mut got_first = false;
        let mut first_open = true;
        let mut samples: VecDeque<(Instant, u64)> = VecDeque::new();
        let mut ticker = tokio::time::interval(Duration::from_secs(1));

        let result = {
            let download = engine::run(&self.client, &job, &cfg, &run.progress, run.stop.clone(), Some(first_tx));
            tokio::pin!(download);
            loop {
                tokio::select! {
                    result = &mut download => break result,
                    signal = &mut first_rx, if first_open => {
                        first_open = false;
                        if signal.is_ok() {
                            got_first = true;
                            run.first.send_replace(First::Got);
                        }
                    }
                    _ = ticker.tick() => {
                        let downloaded = run.progress.downloaded();
                        let at = Instant::now();
                        samples.push_back((at, downloaded));
                        while samples.front().is_some_and(|(t, _)| at.duration_since(*t) > SPEED_WINDOW) {
                            samples.pop_front();
                        }
                        let speed = match samples.front() {
                            Some((t, bytes)) if at > *t => {
                                downloaded.saturating_sub(*bytes) as f64 / at.duration_since(*t).as_secs_f64()
                            }
                            _ => 0.0,
                        };
                        run.speed.store(speed.to_bits(), Ordering::Relaxed);
                        let total = match run.progress.total() { 0 => row.filesize, t => t };
                        let _ = self.db.set_progress(&row.id, downloaded, total, speed, eta(total, downloaded, speed));
                    }
                }
            }
        };

        let downloaded = run.progress.downloaded();
        let total = match run.progress.total() {
            0 => row.filesize,
            t => t,
        };
        let delete = lock(&run.delete).take();
        if let Some(delete_file) = delete {
            self.remove_task(&row, delete_file);
        } else {
            match result {
                Ok(Outcome::Completed { path, size }) => {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let _ = self.db.set_file(&row.id, &path.to_string_lossy(), &name, size);
                    let _ = self.db.set_status(&row.id, "completed", "");
                    let _ = self.db.clear_cookies(&row.id);
                    run.first.send_replace(First::Got);
                }
                Ok(Outcome::Paused) => {
                    let _ = self.db.set_progress(&row.id, downloaded, total, 0.0, 0);
                    let _ = self.db.set_status(&row.id, "paused", "");
                    if !got_first {
                        run.first.send_replace(First::Failed("Unduhan dijeda sebelum dimulai.".into()));
                    }
                }
                Err(error) => {
                    let message = error.user_message();
                    let _ = self.db.set_progress(&row.id, downloaded, total, 0.0, 0);
                    let _ = self.db.set_status(&row.id, "error", &message);
                    if !got_first {
                        run.first.send_replace(First::Failed(message));
                    }
                }
            }
        }
        lock(&self.active).remove(&row.id);
        self.schedule();
    }
}

struct RunCtx {
    stop: CancellationToken,
    progress: Progress,
    speed: Arc<AtomicU64>,
    delete: Arc<Mutex<Option<bool>>>,
    first: Arc<watch::Sender<First>>,
}

fn eta(total: u64, downloaded: u64, speed: f64) -> u64 {
    if speed > 1.0 && total > downloaded {
        ((total - downloaded) as f64 / speed).ceil() as u64
    } else {
        0
    }
}
