//! Commands the UI calls. Everything is validated here; the UI is not trusted.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use openidm_core::db::TaskRow;
use openidm_core::manager::{Manager, NewTask};
use openidm_core::naming::{category_for, http_url, resolve_filename};
#[cfg(test)]
use openidm_core::naming::valid_category;
use openidm_core::settings::Settings;
use serde::Serialize;
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

type Mgr<'a> = State<'a, Arc<Manager>>;

const LEGACY_ONLY: &str = "Tautan halaman ini perlu dianalisis sebagai video. Untuk folder Google Drive, gunakan tautan berkas langsung.";

/// Sites whose pages need the extractor (stage 2/3), not a plain file download.
const CORE_HOSTS: [&str; 11] = [
    "youtube.com", "youtu.be", "tiktok.com", "instagram.com", "twitter.com", "x.com", "dailymotion.com",
    "vimeo.com", "facebook.com", "drive.google.com", "docs.google.com",
];

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Action {
    Pause,
    Resume,
    Delete,
}

pub fn parse_action(action: &str) -> Result<Action, String> {
    match action {
        "pause" | "cancel" => Ok(Action::Pause),
        "resume" => Ok(Action::Resume),
        "delete" => Ok(Action::Delete),
        _ => Err("Perintah tidak dikenal.".into()),
    }
}

#[derive(Serialize, Debug)]
pub struct DirectInfo {
    pub filename: String,
    pub category: &'static str,
    pub direct: bool,
}

/// Accepts only links this app can download as a plain file.
pub fn classify_info(input: &str) -> Result<DirectInfo, String> {
    let url = http_url(input).ok_or("Tautan tidak valid.")?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let needs_extractor = CORE_HOSTS.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")));
    let path = url.path().to_ascii_lowercase();
    if needs_extractor || path.ends_with(".m3u8") || path.ends_with(".mpd") {
        return Err(LEGACY_ONLY.into());
    }
    let filename = resolve_filename(None, None, &url);
    let category = category_for(&filename);
    Ok(DirectInfo { filename, category, direct: true })
}

#[derive(Serialize)]
pub struct Disk {
    free: u64,
    total: u64,
    percent: f64,
}

#[derive(Serialize)]
pub struct Status {
    version: &'static str,
    active_downloads: usize,
    total_downloads: usize,
    disk: Option<Disk>,
}

fn disk_of(dir: &Path) -> Option<Disk> {
    let existing = dir.ancestors().find(|p| p.exists())?;
    let total = fs4::total_space(existing).ok()?;
    let free = fs4::available_space(existing).ok()?;
    let percent = if total > 0 { ((total - free.min(total)) as f64 / total as f64 * 1000.0).round() / 10.0 } else { 0.0 };
    Some(Disk { free, total, percent })
}

#[tauri::command]
pub fn get_status(m: Mgr) -> Status {
    Status {
        version: env!("CARGO_PKG_VERSION"),
        active_downloads: m.active_count(),
        total_downloads: m.list(None, None).len(),
        disk: disk_of(&m.settings().download_dir),
    }
}

#[tauri::command]
pub fn list_downloads(m: Mgr, category: Option<String>, status: Option<String>) -> Vec<TaskRow> {
    m.list(category.as_deref(), status.as_deref())
}

#[tauri::command]
pub async fn add_download(m: Mgr<'_>, url: String, filename: Option<String>, category: Option<String>, format_id: Option<String>) -> Result<TaskRow, String> {
    if openidm_core::video::needs_extractor(&url) {
        let format = format_id.filter(|f| f != "direct").unwrap_or_else(|| "best".into());
        openidm_core::video::selector(&format)?;
        let mut filename = match filename {
            Some(name) => name,
            None => m.video_info(&url).await?.filename,
        };
        let mut path = PathBuf::from(&filename);
        path.set_extension(if format == "audio" { "m4a" } else { "mp4" });
        filename = path.to_string_lossy().into_owned();
        let category = category.filter(|c| c != "General").unwrap_or_else(|| if format == "audio" { "Music" } else { "Video" }.into());
        return m.add(NewTask { url, filename: Some(filename), category: Some(category), format_id: Some(format), ..Default::default() });
    }
    classify_info(&url)?;
    m.add(NewTask { url, filename, category, ..Default::default() })
}

#[tauri::command]
pub fn control_download(m: Mgr, id: String, action: String, delete_file: Option<bool>) -> Result<bool, String> {
    Ok(match parse_action(&action)? {
        Action::Pause => m.pause(&id),
        Action::Resume => m.resume(&id),
        Action::Delete => m.delete(&id, delete_file.unwrap_or(false)),
    })
}

/// Only paths that belong to a known download are ever opened.
fn task_path(m: &Manager, id: &str) -> Result<PathBuf, String> {
    m.get(id).map(|row| PathBuf::from(row.filepath)).ok_or_else(|| "Unduhan tidak ditemukan.".to_string())
}

#[tauri::command]
pub fn open_file(m: Mgr, id: String) -> Result<bool, String> {
    let path = task_path(&m, &id)?;
    if !path.is_file() {
        return Err("Berkas tidak ditemukan. Mungkin sudah dipindah atau dihapus.".into());
    }
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|_| "Berkas tidak bisa dibuka.".to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn open_folder(m: Mgr, id: String) -> Result<bool, String> {
    let path = task_path(&m, &id)?;
    let opened = if path.exists() {
        tauri_plugin_opener::reveal_item_in_dir(&path)
    } else {
        match path.parent().filter(|p| p.is_dir()) {
            Some(parent) => tauri_plugin_opener::open_path(parent, None::<&str>),
            None => return Err("Folder tidak ditemukan.".into()),
        }
    };
    opened.map_err(|_| "Folder tidak bisa dibuka.".to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn open_download_dir(m: Mgr) -> Result<bool, String> {
    let dir = m.settings().download_dir;
    std::fs::create_dir_all(&dir).map_err(|_| format!("Folder unduhan tidak bisa diakses: {}", dir.display()))?;
    tauri_plugin_opener::open_path(&dir, None::<&str>).map_err(|_| "Folder tidak bisa dibuka.".to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn get_info(m: Mgr<'_>, url: String) -> Result<serde_json::Value, String> {
    if openidm_core::video::needs_extractor(&url) {
        return serde_json::to_value(m.video_info(&url).await?).map_err(|_| "Informasi video tidak valid.".into());
    }
    let direct = classify_info(&url)?;
    Ok(serde_json::json!({"url":url,"title":direct.filename,"filename":direct.filename,"category":direct.category,"direct":true,"formats":[]}))
}

#[tauri::command]
pub fn get_settings(m: Mgr) -> Settings {
    m.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, m: Mgr, settings: Settings) -> Result<Settings, String> {
    let saved = m.update_settings(settings)?;
    crate::sync_autostart(&app, saved.autostart);
    Ok(saved)
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title("Pilih folder unduhan").pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    let folder = rx.await.map_err(|_| "Pemilihan folder dibatalkan.".to_string())?;
    Ok(folder.and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().into_owned()))
}

/// Pauses running downloads (saving their position), then exits.
pub async fn quit(app: AppHandle, m: Arc<Manager>) {
    m.shutdown();
    for _ in 0..60 {
        if m.active_count() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    app.exit(0);
}

#[tauri::command]
pub async fn quit_app(app: AppHandle, m: Mgr<'_>) -> Result<(), String> {
    quit(app, m.inner().clone()).await;
    Ok(())
}

#[tauri::command]
pub fn browser_status() -> crate::register::BrowserStatus {
    crate::register::status(&crate::data_dir())
}

#[tauri::command]
pub fn register_browser() -> Result<crate::register::BrowserStatus, String> {
    let dir = crate::data_dir();
    crate::register::register(&dir)?;
    Ok(crate::register::status(&dir))
}

#[tauri::command]
pub fn win_minimize(window: WebviewWindow) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn win_toggle_maximize(window: WebviewWindow) {
    let _ = if window.is_maximized().unwrap_or(false) { window.unmaximize() } else { window.maximize() };
}

#[tauri::command]
pub fn win_hide(window: WebviewWindow) {
    let _ = window.hide();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_action_validation() {
        assert_eq!(parse_action("pause").unwrap(), Action::Pause);
        assert_eq!(parse_action("resume").unwrap(), Action::Resume);
        assert_eq!(parse_action("delete").unwrap(), Action::Delete);
        assert_eq!(parse_action("cancel").unwrap(), Action::Pause);
        assert_eq!(parse_action("rm -rf").unwrap_err(), "Perintah tidak dikenal.");
    }

    #[test]
    fn direct_classification_does_not_mistake_video_pages_for_files() {
        for url in ["https://www.youtube.com/watch?v=x", "https://youtu.be/x", "https://drive.google.com/drive/folders/abc",
            "https://a.com/x.m3u8", "https://a.com/x.mpd?token=1"] { assert!(classify_info(url).is_err()); }
        let info = classify_info("https://a.com/files/setup.exe?v=2").unwrap();
        assert_eq!((info.filename.as_str(), info.category, info.direct), ("setup.exe", "Programs", true));
        assert_eq!(classify_info("notaurl").unwrap_err(), "Tautan tidak valid.");
    }

    #[test]
    fn takeout_and_usercontent_downloads_are_direct_files() {
        // The legacy core mistook this host for a Drive folder page.
        let info = classify_info("https://takeout-download-drive.usercontent.google.com/download/Motorgrader%20Group-001.zip?j=1").unwrap();
        assert_eq!((info.filename.as_str(), info.category), ("Motorgrader Group-001.zip", "Compressed"));
        assert!(classify_info("https://drive.usercontent.google.com/download?id=abc&export=download").is_ok());
    }

    #[test]
    fn category_override_is_allow_listed() {
        assert_eq!(valid_category(Some("Music")), Some("Music"));
        assert_eq!(valid_category(Some("../../Windows")), None);
        assert_eq!(valid_category(Some("")), None);
        assert_eq!(valid_category(None), None);
    }
}
