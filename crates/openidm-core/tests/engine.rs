mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use openidm_core::engine::{build_client, run, EngineConfig, EngineError, Job, Outcome, Progress};
use support::{body, ok, serve, ServeOpts};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

const MB: usize = 1024 * 1024;

fn job(dir: &Path, url: &str, size: u64) -> Job {
    Job {
        id: "t".into(),
        url: url::Url::parse(url).unwrap(),
        headers: BTreeMap::new(),
        part_path: dir.join("out.bin.part"),
        final_path: dir.join("out.bin"),
        expected_size: size,
    }
}

async fn go(j: &Job, cfg: &EngineConfig, p: &Progress) -> Result<Outcome, EngineError> {
    run(&build_client(), j, cfg, p, CancellationToken::new(), None).await
}

/// Runs until `at` bytes arrived, then pauses. Returns the outcome.
async fn run_until(j: &Job, p: &Progress, at: u64) -> Result<Outcome, EngineError> {
    let stop = CancellationToken::new();
    let (s2, p2) = (stop.clone(), p.clone());
    tokio::spawn(async move {
        while p2.downloaded() < at {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        s2.cancel();
    });
    run(&build_client(), j, &EngineConfig::default(), p, stop, None).await
}

fn saved_bytes(dir: &Path) -> u64 {
    let st: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("out.bin.part.json")).unwrap()).unwrap();
    st["segments"].as_array().unwrap().iter().map(|s| s["done"].as_u64().unwrap()).sum()
}

// ---- Task 5 ----

#[tokio::test]
async fn segmented_download_matches_source() {
    let data = body(5 * MB);
    let s = serve(data.clone(), ok()).await;
    let d = tempdir().unwrap();
    let p = Progress::default();
    let out = go(&job(d.path(), &s.url, 0), &EngineConfig::default(), &p).await.unwrap();
    assert_eq!(out, Outcome::Completed { path: d.path().join("out.bin"), size: data.len() as u64 });
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(s.hits.load(Ordering::SeqCst) >= 9, "probe + 8 segments");
    assert!(!d.path().join("out.bin.part").exists() && !d.path().join("out.bin.part.json").exists());
    assert!(p.can_resume());
    assert_eq!(p.downloaded(), data.len() as u64);
    assert_eq!(p.total(), data.len() as u64);
}

#[tokio::test]
async fn small_file_uses_single_stream() {
    let data = body(100_000);
    let s = serve(data.clone(), ok()).await;
    let d = tempdir().unwrap();
    go(&job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(s.hits.load(Ordering::SeqCst) <= 2);
}

#[tokio::test]
async fn no_range_unknown_length() {
    let data = body(3 * MB);
    let s = serve(data.clone(), ServeOpts::default()).await;
    let d = tempdir().unwrap();
    let p = Progress::default();
    go(&job(d.path(), &s.url, 0), &EngineConfig::default(), &p).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(!p.can_resume());
}

#[tokio::test]
async fn rejected_before_first_byte() {
    let s = serve(vec![], ServeOpts { status: Some(403), ..Default::default() }).await;
    let d = tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let e = run(&build_client(), &job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default(), CancellationToken::new(), Some(tx))
        .await
        .unwrap_err();
    assert!(matches!(e, EngineError::Rejected(403)), "{e:?}");
    assert_eq!(e.user_message(), "Server menolak unduhan (HTTP 403).");
    assert!(rx.try_recv().is_err());
    assert!(!d.path().join("out.bin.part").exists());
}

/// Google Drive / Takeout without the session cookie: 302 to accounts.google.com/ServiceLogin, which answers 400.
#[tokio::test]
async fn redirect_to_a_login_page_is_reported_as_login_required() {
    let login = serve(vec![], ServeOpts { status: Some(400), ..Default::default() }).await;
    let file = serve(vec![], ServeOpts {
        redirect_to: Some(format!("{}/ServiceLogin?continue=https%3A%2F%2Fexample", login.url)),
        ..Default::default()
    })
    .await;
    let d = tempdir().unwrap();
    let e = go(&job(d.path(), &file.url, 0), &EngineConfig::default(), &Progress::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::LoginRequired), "{e:?}");
    assert_eq!(
        e.user_message(),
        "Situs meminta login. Unduh ulang lewat tombol unduh di browser agar sesi login ikut terbawa."
    );
    assert!(!d.path().join("out.bin").exists());
}

/// A login page answering 200 must never be saved as the requested file.
#[tokio::test]
async fn login_page_with_ok_status_is_not_saved_as_the_file() {
    let login = serve(b"<html>Sign in</html>".to_vec(), ServeOpts { content_length: true, ..Default::default() }).await;
    let file = serve(vec![], ServeOpts { redirect_to: Some(format!("{}/signin/v2/identifier", login.url)), ..Default::default() }).await;
    let d = tempdir().unwrap();
    let e = go(&job(d.path(), &file.url, 0), &EngineConfig::default(), &Progress::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::LoginRequired), "{e:?}");
    assert!(!d.path().join("out.bin").exists());
}

/// Ordinary redirects (CDN mirrors) keep working.
#[tokio::test]
async fn plain_redirect_to_a_mirror_still_downloads() {
    let data = body(3 * MB);
    let mirror = serve(data.clone(), ok()).await;
    let file = serve(vec![], ServeOpts { redirect_to: Some(format!("{}/files/out.bin", mirror.url)), ..Default::default() }).await;
    let d = tempdir().unwrap();
    go(&job(d.path(), &file.url, 0), &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}

#[tokio::test]
async fn first_byte_signalled() {
    let s = serve(body(4096), ok()).await;
    let d = tempdir().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    run(&build_client(), &job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default(), CancellationToken::new(), Some(tx))
        .await
        .unwrap();
    assert!(rx.await.is_ok());
}

#[tokio::test]
async fn headers_forwarded() {
    let s = serve(body(3 * MB), ok()).await;
    let d = tempdir().unwrap();
    let mut j = job(d.path(), &s.url, 0);
    j.headers.insert("Cookie".into(), "k=v".into());
    j.headers.insert("Referer".into(), "https://ref/".into());
    go(&j, &EngineConfig::default(), &Progress::default()).await.unwrap();
    let h = s.last_headers();
    assert_eq!(h["cookie"], "k=v");
    assert_eq!(h["referer"], "https://ref/");
}

// ---- Task 6 ----

#[tokio::test]
async fn pause_then_resume_completes_without_refetching_all() {
    let data = body(8 * MB);
    let s = serve(data.clone(), ServeOpts { throttle_bps: Some(MB as u64), ..ok() }).await;
    let d = tempdir().unwrap();
    let j = job(d.path(), &s.url, 0);
    let p = Progress::default();
    assert_eq!(run_until(&j, &p, MB as u64).await.unwrap(), Outcome::Paused);
    let saved = saved_bytes(d.path());
    assert!(saved >= MB as u64 && saved < data.len() as u64, "saved {saved}");
    assert!(d.path().join("out.bin.part").exists());

    s.set_opts(ok());
    tokio::time::sleep(Duration::from_millis(200)).await; // let aborted responses stop counting
    let served_before = s.bytes_served();
    let p = Progress::default();
    go(&j, &EngineConfig::default(), &p).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(s.bytes_served() - served_before <= data.len() as u64 - saved + 64 * 1024);
    assert_eq!(p.downloaded(), data.len() as u64);
}

#[tokio::test]
async fn dropped_connections_are_retried() {
    let data = body(4 * MB);
    let s = serve(data.clone(), ServeOpts { drop_after: Some(300_000), ..ok() }).await;
    let d = tempdir().unwrap();
    let cfg = EngineConfig { retries: 20, retry_base: Duration::from_millis(5), ..Default::default() };
    go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}

#[tokio::test]
async fn retries_exhausted_keeps_part_and_state() {
    let s = serve(body(4 * MB), ServeOpts { drop_after: Some(1000), ..ok() }).await;
    let d = tempdir().unwrap();
    let cfg = EngineConfig { retries: 2, retry_base: Duration::from_millis(5), ..Default::default() };
    let e = go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::Network(_)), "{e:?}");
    assert_eq!(e.user_message(), "Koneksi terputus. Coba lanjutkan.");
    assert!(d.path().join("out.bin.part").exists() && d.path().join("out.bin.part.json").exists());
}

#[tokio::test]
async fn expired_link_on_resume() {
    let data = body(8 * MB);
    let s = serve(data, ServeOpts { throttle_bps: Some(MB as u64), ..ok() }).await;
    let d = tempdir().unwrap();
    let j = job(d.path(), &s.url, 0);
    assert_eq!(run_until(&j, &Progress::default(), MB as u64).await.unwrap(), Outcome::Paused);

    s.set_opts(ServeOpts { status_on_range_from: Some((1, 403)), ..ok() });
    let e = go(&j, &EngineConfig::default(), &Progress::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::Expired), "{e:?}");
    assert_eq!(e.user_message(), "Tautan sudah kedaluwarsa. Unduh ulang dari website.");
    assert!(d.path().join("out.bin.part").exists());
    assert!(d.path().join("out.bin.part.json").exists());
}

#[tokio::test]
async fn range_ignored_midway_falls_back_to_single() {
    let data = body(4 * MB);
    let s = serve(data.clone(), ServeOpts { ignore_range_after_probe: true, ..ok() }).await;
    let d = tempdir().unwrap();
    let p = Progress::default();
    go(&job(d.path(), &s.url, 0), &EngineConfig::default(), &p).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert_eq!(p.downloaded(), data.len() as u64);
    assert!(!d.path().join("out.bin.part.json").exists());
}

#[tokio::test]
async fn missing_part_restarts_from_zero() {
    let data = body(8 * MB);
    let s = serve(data.clone(), ServeOpts { throttle_bps: Some(MB as u64), ..ok() }).await;
    let d = tempdir().unwrap();
    let j = job(d.path(), &s.url, 0);
    assert_eq!(run_until(&j, &Progress::default(), MB as u64).await.unwrap(), Outcome::Paused);
    std::fs::remove_file(d.path().join("out.bin.part")).unwrap();
    s.set_opts(ok());
    go(&j, &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}

#[tokio::test]
async fn state_for_other_url_is_ignored() {
    let data = body(3 * MB);
    let s = serve(data.clone(), ok()).await;
    let d = tempdir().unwrap();
    std::fs::write(d.path().join("out.bin.part"), vec![7u8; data.len()]).unwrap();
    let fake = serde_json::json!({ "url": "https://other.example/x", "total": data.len(),
        "segments": [{ "start": 0, "end": data.len() - 1, "done": data.len() }] });
    std::fs::write(d.path().join("out.bin.part.json"), fake.to_string()).unwrap();
    go(&job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}

#[tokio::test]
async fn disk_full_reported() {
    let s = serve(body(1024), ok()).await;
    let d = tempdir().unwrap();
    let cfg = EngineConfig { free_space: Arc::new(|_| Ok(50 * 1024 * 1024)), ..Default::default() };
    let e = go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::DiskFull { free, .. } if free == 50 * 1024 * 1024), "{e:?}");
    assert!(e.user_message().starts_with("Ruang disk tidak cukup di drive"), "{}", e.user_message());
    assert!(!d.path().join("out.bin").exists());
}

#[tokio::test]
async fn single_stream_pause_marks_not_resumable_and_restarts() {
    let data = body(3 * MB);
    let s = serve(data.clone(), ServeOpts { content_length: true, throttle_bps: Some(MB as u64), ..Default::default() }).await;
    let d = tempdir().unwrap();
    let j = job(d.path(), &s.url, 0);
    let p = Progress::default();
    assert_eq!(run_until(&j, &p, 256 * 1024).await.unwrap(), Outcome::Paused);
    assert!(!p.can_resume());
    s.set_opts(ServeOpts { content_length: true, ..Default::default() });
    go(&j, &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}

#[tokio::test]
async fn truncated_single_stream_is_not_reported_complete() {
    let s = serve(body(MB), ServeOpts { content_length: true, drop_after: Some(1000), ..Default::default() }).await;
    let d = tempdir().unwrap();
    let cfg = EngineConfig { retries: 1, retry_base: Duration::from_millis(5), ..Default::default() };
    let e = go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::Network(_)), "{e:?}");
    assert!(!d.path().join("out.bin").exists());
}

#[tokio::test]
async fn unavailable_directory_reported() {
    let s = serve(body(10), ok()).await;
    let d = tempdir().unwrap();
    let blocker = d.path().join("blocked");
    std::fs::write(&blocker, b"x").unwrap();
    let e = go(&job(&blocker, &s.url, 0), &EngineConfig::default(), &Progress::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::DirUnavailable(_)), "{e:?}");
    assert!(e.user_message().starts_with("Folder unduhan tidak bisa diakses:"));
}

// ---- IDM-style transfer ----

const BROWSER_UA_PREFIX: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";

#[tokio::test]
async fn default_user_agent_is_sent_and_a_given_one_is_kept() {
    let s = serve(body(3 * MB), ok()).await;
    let d = tempdir().unwrap();
    go(&job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert!(s.last_headers()["user-agent"].starts_with(BROWSER_UA_PREFIX), "{:?}", s.last_headers().get("user-agent"));

    let d = tempdir().unwrap();
    let mut j = job(d.path(), &s.url, 0);
    j.headers.insert("User-Agent".into(), "BrowserUA/1".into());
    go(&j, &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert_eq!(s.last_headers()["user-agent"], "BrowserUA/1");
}

/// A connection that finishes early takes half of the largest remaining range (dynamic segmentation).
#[tokio::test]
async fn finished_connections_split_the_largest_remaining_segment() {
    let data = body(8 * MB);
    let total = data.len() as u64;
    let quarter = total / 4;
    let s = serve(data.clone(), ServeOpts { throttle_bps: Some(2 * MB as u64), ..ok() }).await;
    let d = tempdir().unwrap();
    // Resume a download whose first three quarters are almost done and whose last quarter barely started.
    let done = [quarter - 64 * 1024, quarter - 64 * 1024, quarter - 64 * 1024, 0];
    let mut part = vec![0u8; data.len()];
    for (i, n) in done.iter().enumerate() {
        let at = i * quarter as usize;
        part[at..at + *n as usize].copy_from_slice(&data[at..at + *n as usize]);
    }
    std::fs::write(d.path().join("out.bin.part"), part).unwrap();
    let state = serde_json::json!({ "url": url::Url::parse(&s.url).unwrap().to_string(), "total": total,
        "segments": (0..4).map(|i| serde_json::json!({ "start": i * quarter, "end": (i + 1) * quarter - 1, "done": done[i as usize] }))
            .collect::<Vec<_>>() });
    std::fs::write(d.path().join("out.bin.part.json"), state.to_string()).unwrap();

    let cfg = EngineConfig { segments: 4, ..Default::default() };
    go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);

    let resumed_at: Vec<u64> = (0..4).map(|i| i * quarter + done[i as usize]).collect();
    let split_starts: Vec<u64> = s
        .ranges_seen()
        .into_iter()
        .map(|(a, _)| a)
        .filter(|a| *a > 3 * quarter && !resumed_at.contains(a))
        .collect();
    assert!(!split_starts.is_empty(), "idle connections should split the last quarter; ranges {:?}", s.ranges_seen());
    assert!(s.peak_connections() <= 4 + 1, "never more connections than configured (+probe)");
}

/// A server that allows fewer connections than configured answers the extras with 429.
#[tokio::test]
async fn throttled_connections_retire_instead_of_failing_the_download() {
    let data = body(6 * MB);
    let s = serve(data.clone(), ServeOpts { max_connections: Some(2), throttle_bps: Some(6 * MB as u64), ..ok() }).await;
    let d = tempdir().unwrap();
    let cfg = EngineConfig { segments: 8, retry_base: Duration::from_millis(5), ..Default::default() };
    go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(s.peak_connections() <= 2);
}

#[tokio::test]
async fn a_server_allowing_one_connection_still_completes() {
    let data = body(3 * MB);
    let s = serve(data.clone(), ServeOpts { max_connections: Some(1), ..ok() }).await;
    let d = tempdir().unwrap();
    let cfg = EngineConfig { segments: 8, retry_base: Duration::from_millis(5), ..Default::default() };
    go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}

#[tokio::test]
async fn throttled_probe_is_retried() {
    let data = body(3 * MB);
    let s = serve(data.clone(), ServeOpts { fail_first: Some((2, 429)), ..ok() }).await;
    let d = tempdir().unwrap();
    let cfg = EngineConfig { retry_base: Duration::from_millis(5), ..Default::default() };
    go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}

#[tokio::test]
async fn persistent_throttling_eventually_fails_with_a_clear_message() {
    let s = serve(body(3 * MB), ServeOpts { fail_first: Some((1000, 503)), ..ok() }).await;
    let d = tempdir().unwrap();
    let cfg = EngineConfig { retries: 2, retry_base: Duration::from_millis(5), ..Default::default() };
    let e = go(&job(d.path(), &s.url, 0), &cfg, &Progress::default()).await.unwrap_err();
    assert!(matches!(e, EngineError::Rejected(503)), "{e:?}");
}

/// Segments are visible while downloading and after a pause; their bytes add up to the progress.
#[tokio::test]
async fn segment_snapshot_matches_progress_after_pause_and_split_state_resumes() {
    let data = body(8 * MB);
    let s = serve(data.clone(), ServeOpts { throttle_bps: Some(MB as u64), ..ok() }).await;
    let d = tempdir().unwrap();
    let j = job(d.path(), &s.url, 0);
    let p = Progress::default();
    assert_eq!(run_until(&j, &p, 2 * MB as u64).await.unwrap(), Outcome::Paused);
    let segments = p.segments();
    assert!(segments.len() >= 8, "{}", segments.len());
    assert_eq!(segments.iter().map(|s| s.done).sum::<u64>(), saved_bytes(d.path()));
    assert!(segments.iter().all(|s| !s.active), "nothing is active after a pause");
    let mut covered: Vec<(u64, u64)> = segments.iter().map(|s| (s.start, s.end)).collect();
    covered.sort();
    assert_eq!(covered.first().unwrap().0, 0);
    assert_eq!(covered.last().unwrap().1, data.len() as u64 - 1);
    assert!(covered.windows(2).all(|w| w[0].1 + 1 == w[1].0), "segments tile the file exactly");

    s.set_opts(ok());
    go(&j, &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}

#[tokio::test]
async fn overlapping_saved_state_is_ignored() {
    let data = body(3 * MB);
    let s = serve(data.clone(), ok()).await;
    let d = tempdir().unwrap();
    let total = data.len() as u64;
    std::fs::write(d.path().join("out.bin.part"), vec![7u8; data.len()]).unwrap();
    let bad = serde_json::json!({ "url": format!("{}/", s.url.trim_end_matches('/')), "total": total, "segments": [
        { "start": 0, "end": total - 1, "done": total / 2 },
        { "start": total / 4, "end": total - 1, "done": total / 2 } ] });
    std::fs::write(d.path().join("out.bin.part.json"), bad.to_string()).unwrap();
    go(&job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default()).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}
