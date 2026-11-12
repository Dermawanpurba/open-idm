mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use openidm_core::db::Db;
use openidm_core::engine::EngineConfig;
use openidm_core::manager::{Manager, NewTask};
use openidm_core::settings::Settings;
use support::{body, ok, serve, ServeOpts};
use tempfile::{tempdir, TempDir};

const MB: usize = 1024 * 1024;

fn fast_cfg() -> EngineConfig {
    EngineConfig { retries: 1, retry_base: Duration::from_millis(5), ..Default::default() }
}

fn open(dir: &Path) -> Arc<Manager> {
    let m = Manager::with_engine_config(dir, fast_cfg()).unwrap();
    m.update_settings(Settings { download_dir: dir.join("dl"), autostart: false, ..m.settings() }).unwrap();
    m
}

fn setup() -> (TempDir, Arc<Manager>) {
    let d = tempdir().unwrap();
    let m = open(d.path());
    (d, m)
}

fn nt() -> NewTask {
    NewTask::default()
}

fn hdr(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn wait_status(m: &Manager, id: &str, status: &str) {
    for _ in 0..1000 {
        if m.get(id).map(|r| r.status == status).unwrap_or(false) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("task {id} never reached {status}: {:?}", m.get(id).map(|r| (r.status, r.error_message)));
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..1000 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn slow() -> ServeOpts {
    ServeOpts { throttle_bps: Some(256 * 1024), ..ok() }
}

#[tokio::test]
async fn add_completes_and_scrubs_cookies() {
    let data = body(300_000);
    let s = serve(data.clone(), ok()).await;
    let (d, m) = setup();
    let t = m
        .add(NewTask { url: format!("{}/report.pdf", s.url), headers: hdr(&[("Cookie", "k=v"), ("X-Evil", "1")]), ..nt() })
        .unwrap();
    assert_eq!((t.category.as_str(), t.format_id.as_str(), t.filename.as_str()), ("Documents", "direct", "report.pdf"));
    assert_eq!(s.last_headers().get("x-evil"), None);
    m.wait_first_byte(&t.id).await.unwrap();
    wait_status(&m, &t.id, "completed").await;
    assert_eq!(s.last_headers()["cookie"], "k=v");
    let r = m.get(&t.id).unwrap();
    assert_eq!(std::fs::read(&r.filepath).unwrap(), data);
    assert!(Path::new(&r.filepath).starts_with(d.path().join("dl").join("Documents")));
    assert!(r.headers.is_empty());
    assert_eq!((r.filesize, r.downloaded_bytes, r.progress), (300_000, 300_000, 100.0));
}

#[tokio::test]
async fn rejects_bad_url() {
    let (_d, m) = setup();
    assert_eq!(m.add(NewTask { url: "file:///c:/x".into(), ..nt() }).unwrap_err(), "Tautan tidak valid.");
    assert!(m.list(None, None).is_empty());
}

#[tokio::test]
async fn traversal_filename_stays_inside() {
    let s = serve(body(10), ok()).await;
    let (d, m) = setup();
    let t = m.add(NewTask { url: s.url.clone(), filename: Some("..\\..\\evil.exe".into()), ..nt() }).unwrap();
    assert!(Path::new(&t.filepath).starts_with(d.path().join("dl")));
    assert_eq!(t.filename, "evil.exe");
}

#[tokio::test]
async fn concurrency_limit_and_promotion() {
    let s = serve(body(6 * MB), slow()).await;
    let (_d, m) = setup();
    let ids: Vec<_> = (0..5).map(|i| m.add(NewTask { url: format!("{}/f{i}.bin", s.url), ..nt() }).unwrap().id).collect();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(m.active_count(), 3);
    assert_eq!(m.list(None, Some("pending")).len(), 2);
    assert_eq!(m.list(None, Some("downloading")).len(), 3);
    let running = m.list(None, Some("downloading"))[0].id.clone();
    assert!(ids.contains(&running));
    assert!(m.pause(&running));
    wait_status(&m, &running, "paused").await;
    wait_until("a queued task is promoted", || m.active_count() == 3 && m.list(None, Some("pending")).len() == 1).await;
    m.shutdown();
}

#[tokio::test]
async fn immediate_task_bypasses_queue() {
    let s = serve(body(6 * MB), slow()).await;
    let (_d, m) = setup();
    for i in 0..3 {
        m.add(NewTask { url: format!("{}/f{i}.bin", s.url), ..nt() }).unwrap();
    }
    let t = m.add(NewTask { url: format!("{}/now.bin", s.url), immediate: true, ..nt() }).unwrap();
    m.wait_first_byte(&t.id).await.unwrap();
    assert_eq!(m.active_count(), 4);
    m.shutdown();
}

#[tokio::test]
async fn failure_before_first_byte() {
    let s = serve(vec![], ServeOpts { status: Some(404), ..Default::default() }).await;
    let (_d, m) = setup();
    let t = m.add(NewTask { url: s.url.clone(), ..nt() }).unwrap();
    assert_eq!(m.wait_first_byte(&t.id).await.unwrap_err(), "Server menolak unduhan (HTTP 404).");
    let r = m.get(&t.id).unwrap();
    assert_eq!((r.status.as_str(), r.error_message.as_str()), ("error", "Server menolak unduhan (HTTP 404)."));
}

#[tokio::test]
async fn start_paused_does_not_download() {
    let s = serve(body(1000), ok()).await;
    let (_d, m) = setup();
    let t = m.add(NewTask { url: s.url.clone(), start_paused: true, ..nt() }).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(m.get(&t.id).unwrap().status, "paused");
    assert_eq!(s.hits.load(Ordering::SeqCst), 0);
    assert!(m.resume(&t.id));
    wait_status(&m, &t.id, "completed").await;
}

#[tokio::test]
async fn restart_marks_running_paused_then_resumes() {
    let data = body(6 * MB);
    let s = serve(data.clone(), slow()).await;
    let d = tempdir().unwrap();
    let m = open(d.path());
    let t = m.add(NewTask { url: format!("{}/big.bin", s.url), ..nt() }).unwrap();
    wait_until("some bytes arrive", || m.get(&t.id).unwrap().downloaded_bytes > 256 * 1024).await;
    m.shutdown();
    wait_until("runner stops", || m.active_count() == 0).await;
    drop(m);
    // Simulate a crash: the row still says it was running.
    Db::open(&d.path().join("downloads.db")).unwrap().set_status(&t.id, "downloading", "").unwrap();

    s.set_opts(ok());
    let m = open(d.path());
    let r = m.get(&t.id).unwrap();
    assert_eq!(r.status, "paused");
    assert!(r.downloaded_bytes > 0);
    let before = s.bytes_served();
    assert!(m.resume(&t.id));
    wait_status(&m, &t.id, "completed").await;
    assert_eq!(std::fs::read(m.get(&t.id).unwrap().filepath).unwrap(), data);
    assert!(s.bytes_served() - before < data.len() as u64, "resume must not refetch everything");
}

#[tokio::test]
async fn delete_removes_partials_keeps_final_unless_asked() {
    let (d, m) = setup();
    let slow_server = serve(body(6 * MB), slow()).await;
    let t = m.add(NewTask { url: format!("{}/part.bin", slow_server.url), ..nt() }).unwrap();
    wait_until("some bytes arrive", || m.get(&t.id).unwrap().downloaded_bytes > 0).await;
    assert!(m.delete(&t.id, false));
    wait_until("row and partial files are gone", || {
        m.get(&t.id).is_none() && std::fs::read_dir(d.path().join("dl").join("General")).unwrap().count() == 0
    })
    .await;
    assert!(!m.delete(&t.id, false));

    let fast = serve(body(1000), ok()).await;
    let keep = m.add(NewTask { url: format!("{}/keep.txt", fast.url), ..nt() }).unwrap();
    let gone = m.add(NewTask { url: format!("{}/gone.txt", fast.url), ..nt() }).unwrap();
    wait_status(&m, &keep.id, "completed").await;
    wait_status(&m, &gone.id, "completed").await;
    assert!(m.delete(&keep.id, false) && m.delete(&gone.id, true));
    assert!(Path::new(&keep.filepath).exists());
    assert!(!Path::new(&gone.filepath).exists());
}

#[tokio::test]
async fn duplicate_name_gets_suffix() {
    let s = serve(body(1000), ok()).await;
    let (_d, m) = setup();
    let a = m.add(NewTask { url: format!("{}/a.zip", s.url), start_paused: true, ..nt() }).unwrap();
    let b = m.add(NewTask { url: format!("{}/a.zip", s.url), start_paused: true, ..nt() }).unwrap();
    assert_eq!((a.filename.as_str(), b.filename.as_str()), ("a.zip", "a (1).zip"));
    assert_ne!(a.filepath, b.filepath);
    assert_eq!(a.category, "Compressed");
}

#[tokio::test]
async fn category_override_is_honoured_only_when_known() {
    let s = serve(body(10), ok()).await;
    let (d, m) = setup();
    let chosen = m
        .add(NewTask { url: format!("{}/song.zip", s.url), category: Some("Music".into()), start_paused: true, ..nt() })
        .unwrap();
    assert_eq!(chosen.category, "Music");
    assert!(Path::new(&chosen.filepath).starts_with(d.path().join("dl").join("Music")));
    let bogus = m
        .add(NewTask { url: format!("{}/b.zip", s.url), category: Some("..\\..\\Windows".into()), start_paused: true, ..nt() })
        .unwrap();
    assert_eq!(bogus.category, "Compressed");
    assert!(Path::new(&bogus.filepath).starts_with(d.path().join("dl").join("Compressed")));
}

#[tokio::test]
async fn segments_are_reported_while_running_and_after_a_pause() {
    let s = serve(body(8 * MB), slow()).await;
    let (_d, m) = setup();
    let t = m.add(NewTask { url: format!("{}/big.bin", s.url), ..nt() }).unwrap();
    wait_until("several segments are active", || {
        m.get(&t.id).map(|r| r.segments.iter().filter(|s| s.active).count() >= 2).unwrap_or(false)
    })
    .await;
    let running = m.get(&t.id).unwrap();
    assert_eq!(running.segments.first().unwrap().start, 0);
    assert_eq!(running.segments.last().unwrap().end, 8 * MB as u64 - 1);
    let json = serde_json::to_value(&running).unwrap();
    assert!(json["segments"][0]["speed"].is_number(), "segments reach the UI: {json}");

    assert!(m.pause(&t.id));
    wait_status(&m, &t.id, "paused").await;
    let paused = m.get(&t.id).unwrap();
    assert!(!paused.segments.is_empty(), "a paused download still shows its segments");
    assert!(paused.segments.iter().all(|s| !s.active && s.speed == 0.0));
    assert_eq!(paused.segments.iter().map(|s| s.done).sum::<u64>(), paused.downloaded_bytes);
}

#[tokio::test]
async fn finished_and_single_stream_rows_have_no_segments() {
    let s = serve(body(1000), ok()).await;
    let (_d, m) = setup();
    let t = m.add(NewTask { url: s.url.clone(), ..nt() }).unwrap();
    wait_status(&m, &t.id, "completed").await;
    assert!(m.get(&t.id).unwrap().segments.is_empty());
    assert!(serde_json::to_value(m.get(&t.id).unwrap()).unwrap().get("segments").is_none());
}

#[tokio::test]
async fn download_dir_unavailable() {
    let s = serve(body(10), ok()).await;
    let (d, m) = setup();
    let blocker = d.path().join("blocked");
    std::fs::write(&blocker, b"x").unwrap();
    m.update_settings(Settings { download_dir: blocker.clone(), ..m.settings() }).unwrap();
    let t = m.add(NewTask { url: s.url.clone(), ..nt() }).unwrap();
    wait_status(&m, &t.id, "error").await;
    assert!(m.get(&t.id).unwrap().error_message.starts_with("Folder unduhan tidak bisa diakses:"));
}

#[tokio::test]
async fn pause_all_and_resume_all() {
    let data = body(3 * MB);
    let s = serve(data.clone(), slow()).await;
    let (_d, m) = setup();
    let a = m.add(NewTask { url: format!("{}/a.bin", s.url), ..nt() }).unwrap();
    let b = m.add(NewTask { url: format!("{}/b.bin", s.url), ..nt() }).unwrap();
    wait_until("both started", || m.active_count() == 2).await;
    m.pause_all();
    wait_status(&m, &a.id, "paused").await;
    wait_status(&m, &b.id, "paused").await;
    s.set_opts(ok());
    m.resume_all();
    wait_status(&m, &a.id, "completed").await;
    wait_status(&m, &b.id, "completed").await;
    assert_eq!(std::fs::read(m.get(&a.id).unwrap().filepath).unwrap(), data);
}

#[tokio::test]
async fn settings_are_validated_and_persisted() {
    let d = tempdir().unwrap();
    let m = open(d.path());
    assert!(m.update_settings(Settings { max_concurrent: 0, ..m.settings() }).is_err());
    m.update_settings(Settings { max_concurrent: 5, ..m.settings() }).unwrap();
    drop(m);
    assert_eq!(Manager::with_engine_config(d.path(), fast_cfg()).unwrap().settings().max_concurrent, 5);
}
