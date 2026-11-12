//! Portable video extraction and transfer, owned by the Rust queue (no legacy HTTP server).
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::engine::{EngineError, Job, Outcome, Progress};
use crate::naming::{http_url, safe_filename};

const FILE_MARKER: &str = "__OPENIDM_FILE__";
const PROGRESS_MARKER: &str = "__OPENIDM_PROGRESS__";
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn needs_extractor(input: &str) -> bool {
    let Some(url) = http_url(input) else { return false };
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let sites = ["youtube.com", "youtu.be", "youtube-nocookie.com", "tiktok.com", "instagram.com",
        "twitter.com", "x.com", "dailymotion.com", "vimeo.com", "facebook.com", "ok.ru", "bilibili.tv"];
    sites.iter().any(|s| host == *s || host.ends_with(&format!(".{s}"))) ||
        url.path().to_ascii_lowercase().ends_with(".m3u8") || url.path().to_ascii_lowercase().ends_with(".mpd")
}

#[derive(Clone)]
pub struct Tools { pub bin: PathBuf }

impl Tools {
    pub fn at(data_dir: &Path) -> Self { Self { bin: data_dir.join("bin") } }

    fn command(&self) -> Result<Command, String> {
        for file in ["yt-dlp.exe", "ffmpeg.exe", "node.exe"] {
            if !self.bin.join(file).is_file() {
                return Err(format!("Helper video {file} tidak ditemukan. Salin seluruh paket OpenIDM, termasuk folder bin."));
            }
        }
        let mut command = Command::new(self.bin.join("yt-dlp.exe"));
        command.args(["--ignore-config", "--no-plugin-dirs", "--no-cache-dir", "--no-playlist", "--encoding", "utf-8",
            "--socket-timeout", "20", "--retries", "3", "--windows-filenames"])
            .arg("--ffmpeg-location").arg(&self.bin)
            .arg("--js-runtimes").arg(format!("node:{}", self.bin.join("node.exe").display()))
            .stdin(Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        Ok(command)
    }

    pub async fn info(&self, input: &str) -> Result<VideoInfo, String> {
        let url = http_url(input).ok_or("Tautan video tidak valid.")?;
        let mut command = self.command()?;
        command.args(["--dump-single-json", "--skip-download", "--no-warnings", "--"]).arg(url.as_str())
            .stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|_| "Helper video tidak bisa dijalankan.".to_string())?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let read_out = tokio::spawn(async move { let mut bytes = Vec::new(); stdout.take(16 * 1024 * 1024).read_to_end(&mut bytes).await.map(|_| bytes) });
        let read_err = tokio::spawn(async move { let mut bytes = Vec::new(); stderr.take(65536).read_to_end(&mut bytes).await.map(|_| bytes) });
        let status = match tokio::time::timeout(Duration::from_secs(120), child.wait()).await {
            Ok(Ok(status)) => status,
            _ => { terminate(&mut child).await; return Err("Analisis video melewati batas waktu. Coba lagi.".into()); }
        };
        let output = read_out.await.map_err(|_| "Analisis video gagal.")?.map_err(|_| "Analisis video gagal.")?;
        let errors = read_err.await.ok().and_then(Result::ok).unwrap_or_default();
        if !status.success() { return Err(helper_error(&String::from_utf8_lossy(&errors))); }
        let value: Value = serde_json::from_slice(&output).map_err(|_| "Informasi video tidak valid.".to_string())?;
        parse_info(&value, url.as_str())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn download(&self, job: &Job, format: &str, connections: usize, progress: &Progress, stop: CancellationToken,
        mut first: Option<oneshot::Sender<()>>) -> Result<Outcome, EngineError> {
        let selector = selector(format).map_err(EngineError::Video)?;
        let work = work_dir(&job.final_path, &job.id).ok_or_else(|| EngineError::DirUnavailable(job.final_path.clone()))?;
        tokio::fs::create_dir_all(&work).await.map_err(|e| EngineError::Io(e.to_string()))?;
        let mut command = self.command().map_err(EngineError::Video)?;
        command.args(["--newline", "--progress", "--progress-delta", "0.25", "--no-simulate", "--format"])
            .arg(selector).arg("--output").arg(work.join("media.%(ext)s"))
            .args(["--progress-template", "download:__OPENIDM_PROGRESS__%(info.format_id)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s",
                "--print", "after_move:__OPENIDM_FILE__%(filepath)s"])
            .args(parallel_args(connections));
        if format == "audio" { command.args(["--extract-audio", "--audio-format", "m4a"]); }
        else { command.args(["--merge-output-format", "mp4", "--remux-video", "mp4"]); }
        for (name, value) in crate::protocol::filter_headers(&job.headers) {
            if name != "Cookie" { command.arg("--add-header").arg(format!("{name}:{value}")); }
        }
        command.arg("--").arg(job.url.as_str()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|_| EngineError::Video("Helper video tidak bisa dijalankan.".into()))?;
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let stderr = child.stderr.take().unwrap();
        let errors = tokio::spawn(async move { let mut bytes = Vec::new(); stderr.take(65536).read_to_end(&mut bytes).await.map(|_| bytes) });
        let mut output = None;
        let mut tracks = BTreeMap::new();
        progress.set_can_resume(true);
        loop {
            tokio::select! {
                _ = stop.cancelled() => { terminate(&mut child).await; return Ok(Outcome::Paused); }
                line = lines.next_line() => match line {
                    Ok(Some(line)) => {
                        if let Some(path) = line.strip_prefix(FILE_MARKER) { output = Some(PathBuf::from(path)); }
                        if let Some((id, done, total)) = parse_progress(&line) {
                            tracks.insert(id, (done, total));
                            let (done, total) = tracks.values().fold((0, 0), |(a,b),(d,t)| (a+d, b+t));
                            progress.set_downloaded(done); progress.set_total(total);
                            if done > 0 { if let Some(tx) = first.take() { let _ = tx.send(()); } }
                        }
                    }
                    Ok(None) => break,
                    Err(error) => { terminate(&mut child).await; return Err(EngineError::Io(error.to_string())); }
                }
            }
        }
        let status = tokio::select! {
            _ = stop.cancelled() => { terminate(&mut child).await; return Ok(Outcome::Paused); }
            result = child.wait() => result.map_err(|e| EngineError::Io(e.to_string()))?,
        };
        let error = errors.await.ok().and_then(Result::ok).unwrap_or_default();
        if !status.success() { return Err(EngineError::Video(helper_error(&String::from_utf8_lossy(&error)))); }
        let output = output.ok_or_else(|| EngineError::Video("Helper video tidak menghasilkan berkas akhir.".into()))?;
        let canonical = tokio::fs::canonicalize(&output).await.map_err(|e| EngineError::Io(e.to_string()))?;
        let allowed = tokio::fs::canonicalize(&work).await.map_err(|e| EngineError::Io(e.to_string()))?;
        if !canonical.starts_with(&allowed) { return Err(EngineError::Video("Lokasi hasil video tidak valid.".into())); }
        let metadata = tokio::fs::metadata(&canonical).await.map_err(|e| EngineError::Io(e.to_string()))?;
        if !metadata.is_file() { return Err(EngineError::Video("Hasil video bukan berkas.".into())); }
        let size = metadata.len();
        if size == 0 { return Err(EngineError::Video("Berkas video kosong.".into())); }
        let mut attempt = 0;
        loop {
            match tokio::fs::rename(&canonical, &job.final_path).await {
                Ok(()) => break,
                Err(error) if attempt < 8 && error.kind() == std::io::ErrorKind::PermissionDenied => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(error) => return Err(EngineError::Io(error.to_string())),
            }
        }
        let _ = tokio::fs::remove_file(&job.part_path).await;
        let _ = tokio::fs::remove_dir(&work).await;
        progress.set_downloaded(size); progress.set_total(size);
        Ok(Outcome::Completed { path: job.final_path.clone(), size })
    }
}

/// HLS/DASH streams are many small fragments; fetching several at once is what makes them fast.
fn parallel_args(connections: usize) -> [String; 2] {
    ["--concurrent-fragments".into(), connections.clamp(1, 16).to_string()]
}

fn work_dir(final_path: &Path, id: &str) -> Option<PathBuf> {
    Some(final_path.parent()?.join(format!(".openidm-video-{}", safe_filename(id))))
}

pub fn remove_partials(final_path: &Path, id: &str) {
    let Some(work) = work_dir(final_path, id) else { return };
    let (Ok(actual), Some(parent)) = (work.canonicalize(), final_path.parent()) else { return };
    let Ok(parent) = parent.canonicalize() else { return };
    // Only this task's immediate work folder may be removed; never follow an outside junction.
    if actual.parent() == Some(parent.as_path()) { let _ = std::fs::remove_dir_all(work); }
}

async fn terminate(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() { return; }
    #[cfg(windows)]
    if let Some(id) = child.id() {
        let tool = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32/taskkill.exe");
        let _ = Command::new(tool).args(["/PID", &id.to_string(), "/T", "/F"]).creation_flags(CREATE_NO_WINDOW).output().await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[derive(Serialize, Debug, Clone)]
pub struct VideoFormat {
    pub format_id: String, pub quality: String, pub resolution: String, pub ext: String,
    pub filesize: u64, pub filesize_formatted: String, pub has_video: bool, pub has_audio: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct VideoInfo {
    pub url: String, pub title: String, pub filename: String, pub category: String, pub thumbnail: String,
    pub duration: u64, pub duration_formatted: String, pub filesize: u64, pub filesize_formatted: String,
    pub is_stream: bool, pub is_folder: bool, pub direct: bool, pub formats: Vec<VideoFormat>,
}

pub fn parse_info(value: &Value, url: &str) -> Result<VideoInfo, String> {
    if matches!(value["_type"].as_str(), Some("playlist" | "multi_video")) { return Err("Masukkan tautan satu video, bukan playlist.".into()); }
    let title = value["title"].as_str().filter(|s| !s.is_empty()).ok_or("Judul video tidak ditemukan.")?.to_string();
    let mut heights = Vec::new();
    let mut audio = false;
    for format in value["formats"].as_array().into_iter().flatten() {
        if format["has_drm"].as_bool() == Some(true) { continue; }
        if format["vcodec"].as_str().is_some_and(|s| s != "none") {
            if let Some(height) = format["height"].as_u64() { if height > 0 && height <= 4320 { heights.push(height); } }
        }
        if format["acodec"].as_str().is_some_and(|s| s != "none") { audio = true; }
    }
    heights.sort_unstable(); heights.dedup(); heights.reverse();
    let mut formats = vec![make_format("best", "Kualitas terbaik + audio", "Terbaik", "mp4", true)];
    for height in heights { formats.push(make_format(&format!("video:{height}"), &format!("{height}p + audio"), &format!("{height}p"), "mp4", true)); }
    if audio { formats.push(make_format("audio", "Audio saja (M4A)", "Audio", "m4a", false)); }
    let duration = value["duration"].as_f64().unwrap_or(0.0).max(0.0) as u64;
    let filename = format!("{}.mp4", safe_filename(&title.replace(['/', '\\'], "")));
    Ok(VideoInfo { url: url.into(), filename, title, category: "Video".into(),
        thumbnail: value["thumbnail"].as_str().unwrap_or("").into(), duration,
        duration_formatted: format!("{}:{:02}:{:02}", duration / 3600, duration / 60 % 60, duration % 60),
        filesize: 0, filesize_formatted: "Ukuran mengikuti kualitas".into(),
        is_stream: true, is_folder: false, direct: false, formats })
}

fn make_format(id: &str, name: &str, resolution: &str, ext: &str, video: bool) -> VideoFormat {
    VideoFormat { format_id: id.into(), quality: name.into(), resolution: resolution.into(), ext: ext.into(),
        filesize: 0, filesize_formatted: "Adaptive".into(), has_video: video, has_audio: true }
}

pub fn selector(format: &str) -> Result<String, String> {
    if matches!(format, "best" | "video:best") { return Ok("bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/bv*+ba/b".into()); }
    if format == "audio" { return Ok("bestaudio/best".into()); }
    let height = format.strip_prefix("video:").and_then(|s| s.parse::<u32>().ok()).filter(|h| (1..=4320).contains(h))
        .ok_or_else(|| "Pilihan kualitas video tidak valid. Analisis URL kembali.".to_string())?;
    Ok(format!("bv*[height<={height}][ext=mp4]+ba[ext=m4a]/b[height<={height}][ext=mp4]/bv*[height<={height}]+ba/b[height<={height}]"))
}

fn parse_progress(line: &str) -> Option<(String, u64, u64)> {
    let mut fields = line.strip_prefix(PROGRESS_MARKER)?.split('|');
    let id = fields.next()?.to_string();
    let number = |s: &str| s.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0).map(|n| n as u64);
    let done = number(fields.next()?)?;
    let total = number(fields.next().unwrap_or("NA")).or_else(|| number(fields.next().unwrap_or("NA"))).unwrap_or(done);
    Some((id, done, total.max(done)))
}

fn helper_error(stderr: &str) -> String {
    let message = stderr.lines().rev().find(|s| s.contains("ERROR:")).unwrap_or("Video tidak bisa diakses. Periksa tautan dan akses videonya.");
    message.trim().chars().filter(|c| !c.is_control()).take(600).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test] fn selectors_are_validated_and_keep_audio() {
        assert!(selector("video:720").unwrap().contains("+ba"));
        for bad in ["137+140", "video:99999", "--exec", "video:0", "video:1;exit"] { assert!(selector(bad).is_err()); }
        assert_eq!(selector("audio").unwrap(), "bestaudio/best");
    }
    #[test] fn metadata_has_real_quality_choices_and_no_drm_formats() {
        let info = parse_info(&json!({"title":"Demo/Video","duration":61,"formats":[
            {"height":720,"vcodec":"h264","acodec":"none"},{"height":720,"vcodec":"vp9"},
            {"height":1080,"vcodec":"h264","has_drm":true},{"vcodec":"none","acodec":"aac"}]}), "https://youtu.be/test").unwrap();
        assert_eq!(info.filename,"DemoVideo.mp4");
        assert_eq!(info.formats.iter().map(|f| f.format_id.as_str()).collect::<Vec<_>>(), vec!["best","video:720","audio"]);
        assert!(info.formats.iter().all(|f| f.has_audio));
    }
    #[test] fn progress_handles_unknown_totals_and_bad_lines() {
        assert_eq!(parse_progress("__OPENIDM_PROGRESS__137|100|NA|250"),Some(("137".into(),100,250)));
        assert!(parse_progress("__OPENIDM_PROGRESS__137|NA|NA|NA").is_none());
        assert!(parse_progress("random text").is_none());
    }
    #[test] fn fragments_are_fetched_in_parallel_within_safe_bounds() {
        assert_eq!(parallel_args(8), ["--concurrent-fragments", "8"]);
        assert_eq!(parallel_args(0), ["--concurrent-fragments", "1"]);
        assert_eq!(parallel_args(64), ["--concurrent-fragments", "16"]);
    }
    #[test] fn extractor_hosts_use_exact_domain_matching() {
        assert!(needs_extractor("https://www.youtube.com/watch?v=test"));
        assert!(needs_extractor("https://cdn.test/master.m3u8?token=x"));
        assert!(!needs_extractor("https://youtube.com.evil.test/file.zip"));
    }
}
