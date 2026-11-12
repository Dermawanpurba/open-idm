//! Decides what happens to a request from the browser extension. No Tauri types here, so it is testable.

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use openidm_core::manager::{Manager, NewTask};
use openidm_core::naming::{category_for, http_url, resolve_filename};
use openidm_core::protocol::{CaptureItem, Decision, Request, Response};

const INVALID_URL: &str = "Tautan tidak valid.";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Choice {
    Start,
    Later,
    Cancel,
}

/// What the confirmation dialog shows.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Suggested {
    pub filename: String,
    pub category: &'static str,
    pub dir: PathBuf,
    /// Site the file comes from (host only; the full URL may carry tokens).
    pub host: String,
    pub filesize: u64,
}

/// Asks the user what to do with a download taken over from the browser.
pub trait Prompter: Send + Sync {
    /// Returns the choice and, when the user edited it, the new filename.
    fn ask(&self, key: &str, suggested: &Suggested) -> impl Future<Output = (Choice, Option<String>)> + Send;
}

pub trait UiHooks: Send + Sync {
    /// Brings the main window forward, optionally opening "Tambah URL" with `add_url` filled in.
    fn show_main(&self, add_url: Option<String>);
}

static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

fn new_task(item: &CaptureItem, url: &url::Url, filename: Option<String>, request_id: &str) -> NewTask {
    let mut headers = item.headers.clone();
    let has_referer = headers.keys().any(|k| k.eq_ignore_ascii_case("referer"));
    if let (false, Some(referrer)) = (has_referer, item.referrer.as_deref().and_then(http_url)) {
        headers.insert("Referer".into(), referrer.to_string());
    }
    NewTask {
        url: url.to_string(),
        filename,
        filesize: item.filesize,
        headers,
        request_id: Some(request_id.to_string()),
        ..Default::default()
    }
}

/// Handles one request from the browser extension and produces its reply.
pub async fn handle<P: Prompter, U: UiHooks>(request: Request, m: &Arc<Manager>, prompter: &P, ui: &U) -> Response {
    match request {
        Request::Ping { id } => Response {
            id,
            ok: true,
            version: Some(env!("CARGO_PKG_VERSION").into()),
            capabilities: vec!["capture".into(), "add".into(), "show".into()],
            ..Default::default()
        },
        Request::Show { id, add_url } => {
            ui.show_main(add_url.as_deref().and_then(http_url).map(|u| u.to_string()));
            Response { id, ok: true, ..Default::default() }
        }
        Request::Add { id, item } => {
            let Some(url) = http_url(&item.url) else { return Response::error(id, INVALID_URL) };
            match m.add(new_task(&item, &url, item.filename.clone(), &id)) {
                Ok(row) => {
                    ui.show_main(None);
                    Response { id, ok: true, task_id: Some(row.id), ..Default::default() }
                }
                Err(message) => Response::error(id, message),
            }
        }
        Request::Capture { id, item } => {
            let Some(url) = http_url(&item.url) else { return Response::error(id, INVALID_URL) };
            let settings = m.settings();
            let filename = resolve_filename(item.filename.as_deref(), None, &url);
            let (choice, edited) = if settings.confirm_dialog {
                let category = category_for(&filename);
                let suggested = Suggested {
                    dir: settings.download_dir.join(category),
                    category,
                    host: match url.port() {
                        Some(port) => format!("{}:{port}", url.host_str().unwrap_or("")),
                        None => url.host_str().unwrap_or("").to_string(),
                    },
                    filesize: item.filesize,
                    filename: filename.clone(),
                };
                let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed).to_string();
                prompter.ask(&key, &suggested).await
            } else {
                (Choice::Start, None)
            };
            let decided = |decision| Response { id: id.clone(), ok: true, decision: Some(decision), ..Default::default() };
            let mut task = new_task(&item, &url, Some(edited.unwrap_or(filename)), &id);
            match choice {
                Choice::Cancel => decided(Decision::Declined),
                Choice::Later => {
                    task.start_paused = true;
                    match m.add(task) {
                        Ok(row) => Response { task_id: Some(row.id), ..decided(Decision::Later) },
                        Err(message) => Response::error(id, message),
                    }
                }
                Choice::Start => {
                    // Skip the queue: the browser copy stays paused until this is confirmed.
                    task.immediate = true;
                    let row = match m.add(task) {
                        Ok(row) => row,
                        Err(message) => return Response::error(id, message),
                    };
                    ui.show_main(None);
                    match m.wait_first_byte(&row.id).await {
                        Ok(()) => Response { task_id: Some(row.id), ..decided(Decision::Accepted) },
                        Err(message) => {
                            // The browser takes the download back, so leave no failed entry here.
                            m.delete(&row.id, false);
                            Response { error: Some(message), ..decided(Decision::Failed) }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../crates/openidm-core/tests/support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use openidm_core::engine::EngineConfig;
    use openidm_core::settings::Settings;
    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};
    use tokio::sync::Notify;

    use super::support::{body, ok, serve, ServeOpts};
    use super::*;

    /// Answers by filename; an answer can be held back until `release(filename)`.
    #[derive(Default)]
    struct FakePrompter {
        answers: Mutex<HashMap<String, (Choice, Option<String>)>>,
        gates: Mutex<HashMap<String, Arc<Notify>>>,
        calls: AtomicUsize,
        seen: Mutex<Vec<Suggested>>,
    }

    impl FakePrompter {
        fn answering(pairs: &[(&str, Choice)]) -> Self {
            let p = FakePrompter::default();
            for (name, choice) in pairs {
                p.answers.lock().unwrap().insert(name.to_string(), (*choice, None));
            }
            p
        }
        fn hold(&self, name: &str) {
            self.gates.lock().unwrap().insert(name.to_string(), Arc::new(Notify::new()));
        }
        fn release(&self, name: &str) {
            self.gates.lock().unwrap()[name].notify_one();
        }
    }

    impl Prompter for FakePrompter {
        async fn ask(&self, _key: &str, suggested: &Suggested) -> (Choice, Option<String>) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen.lock().unwrap().push(suggested.clone());
            let gate = self.gates.lock().unwrap().get(&suggested.filename).cloned();
            if let Some(gate) = gate {
                gate.notified().await;
            }
            self.answers.lock().unwrap().get(&suggested.filename).cloned().unwrap_or((Choice::Cancel, None))
        }
    }

    #[derive(Default)]
    struct FakeUi(Mutex<Vec<Option<String>>>);
    impl UiHooks for FakeUi {
        fn show_main(&self, add_url: Option<String>) {
            self.0.lock().unwrap().push(add_url);
        }
    }

    fn setup() -> (TempDir, Arc<Manager>) {
        let d = tempdir().unwrap();
        let cfg = EngineConfig { retries: 1, retry_base: Duration::from_millis(5), ..Default::default() };
        let m = Manager::with_engine_config(d.path(), cfg).unwrap();
        m.update_settings(Settings { download_dir: d.path().join("dl"), ..m.settings() }).unwrap();
        (d, m)
    }

    fn capture(id: &str, url: &str) -> Request {
        serde_json::from_value(json!({ "type": "capture", "id": id, "url": url, "filesize": 10 })).unwrap()
    }

    fn js(r: &Response) -> Value {
        serde_json::to_value(r).unwrap()
    }

    #[tokio::test]
    async fn start_returns_accepted_after_first_byte() {
        let s = serve(body(300_000), ok()).await;
        let (_d, m) = setup();
        let p = FakePrompter::answering(&[("a.zip", Choice::Start)]);
        let ui = FakeUi::default();
        let r = handle(capture("1", &format!("{}/a.zip", s.url)), &m, &p, &ui).await;
        assert_eq!(js(&r)["decision"], "accepted");
        assert_eq!(js(&r)["id"], "1");
        assert_eq!(js(&r)["ok"], true);
        let rows = m.list(None, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(js(&r)["task_id"], rows[0].id);
        assert_eq!(*ui.0.lock().unwrap(), vec![None], "the main window comes forward to show progress");
        let seen = p.seen.lock().unwrap();
        assert_eq!((seen[0].filename.as_str(), seen[0].category, seen[0].filesize), ("a.zip", "Compressed", 10));
        assert!(seen[0].host.starts_with("127.0.0.1"));
    }

    #[tokio::test]
    async fn start_but_server_rejects_returns_failed_and_removes_task() {
        let s = serve(vec![], ServeOpts { status: Some(403), ..Default::default() }).await;
        let (_d, m) = setup();
        let p = FakePrompter::answering(&[("a.zip", Choice::Start)]);
        let r = handle(capture("1", &format!("{}/a.zip", s.url)), &m, &p, &FakeUi::default()).await;
        assert_eq!(js(&r)["ok"], true);
        assert_eq!(js(&r)["decision"], "failed");
        assert_eq!(js(&r)["error"], "Server menolak unduhan (HTTP 403).");
        for _ in 0..100 {
            if m.list(None, None).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(m.list(None, None).is_empty(), "a download handed back to the browser leaves no entry behind");
    }

    #[tokio::test]
    async fn later_creates_paused_task() {
        let s = serve(body(1000), ok()).await;
        let (_d, m) = setup();
        let p = FakePrompter::answering(&[("a.zip", Choice::Later)]);
        let r = handle(capture("1", &format!("{}/a.zip", s.url)), &m, &p, &FakeUi::default()).await;
        assert_eq!(js(&r)["decision"], "later");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(m.list(None, None)[0].status, "paused");
        assert_eq!(s.hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancel_declines_and_creates_nothing() {
        let s = serve(body(1000), ok()).await;
        let (_d, m) = setup();
        let p = FakePrompter::answering(&[("a.zip", Choice::Cancel)]);
        let r = handle(capture("1", &format!("{}/a.zip", s.url)), &m, &p, &FakeUi::default()).await;
        assert_eq!(js(&r)["decision"], "declined");
        assert!(m.list(None, None).is_empty());
        assert_eq!(s.hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn edited_filename_is_used_and_sanitized() {
        let s = serve(body(1000), ok()).await;
        let (d, m) = setup();
        let p = FakePrompter::default();
        p.answers.lock().unwrap().insert("a.zip".into(), (Choice::Later, Some("..\\..\\baru.zip".into())));
        handle(capture("1", &format!("{}/a.zip", s.url)), &m, &p, &FakeUi::default()).await;
        let row = &m.list(None, None)[0];
        assert_eq!(row.filename, "baru.zip");
        assert!(std::path::Path::new(&row.filepath).starts_with(d.path().join("dl")));
    }

    #[tokio::test]
    async fn confirm_disabled_skips_prompt() {
        let s = serve(body(1000), ok()).await;
        let (_d, m) = setup();
        m.update_settings(Settings { confirm_dialog: false, ..m.settings() }).unwrap();
        let p = FakePrompter::default();
        let r = handle(capture("1", &format!("{}/a.zip", s.url)), &m, &p, &FakeUi::default()).await;
        assert_eq!(js(&r)["decision"], "accepted");
        assert_eq!(p.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn two_captures_answered_independently() {
        let s = serve(body(300_000), ok()).await;
        let (_d, m) = setup();
        let p = Arc::new(FakePrompter::answering(&[("a.zip", Choice::Start), ("b.zip", Choice::Cancel)]));
        p.hold("a.zip");
        p.hold("b.zip");
        let ui = Arc::new(FakeUi::default());
        let spawn = |id: &'static str, name: &'static str| {
            let (m, p, ui, url) = (m.clone(), p.clone(), ui.clone(), format!("{}/{name}", s.url));
            tokio::spawn(async move { handle(capture(id, &url), &m, &*p, &*ui).await })
        };
        let a = spawn("A", "a.zip");
        let b = spawn("B", "b.zip");
        while p.calls.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        p.release("b.zip");
        let resp_b = b.await.unwrap();
        assert_eq!((resp_b.id.as_str(), js(&resp_b)["decision"].as_str().unwrap()), ("B", "declined"));
        assert!(!a.is_finished(), "A is still waiting for its own answer");
        p.release("a.zip");
        let resp_a = a.await.unwrap();
        assert_eq!((resp_a.id.as_str(), js(&resp_a)["decision"].as_str().unwrap()), ("A", "accepted"));
        assert_eq!(m.list(None, None).len(), 1);
    }

    #[tokio::test]
    async fn invalid_url_rejected_without_prompt() {
        let (_d, m) = setup();
        let p = FakePrompter::default();
        for url in ["javascript:alert(1)", "file:///C:/Windows/win.ini", "https://user:pw@a.com/f.zip"] {
            let r = handle(capture("1", url), &m, &p, &FakeUi::default()).await;
            assert_eq!(js(&r), json!({ "id": "1", "ok": false, "error": "Tautan tidak valid." }));
        }
        assert_eq!(p.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ping_add_and_show() {
        let s = serve(body(1000), ok()).await;
        let (_d, m) = setup();
        let (p, ui) = (FakePrompter::default(), FakeUi::default());

        let ping = handle(serde_json::from_value(json!({ "type": "ping", "id": "p" })).unwrap(), &m, &p, &ui).await;
        assert_eq!(js(&ping)["ok"], true);
        assert_eq!(js(&ping)["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(js(&ping)["capabilities"], json!(["capture", "add", "show"]));

        let add = handle(
            serde_json::from_value(json!({ "type": "add", "id": "a", "url": format!("{}/doc.pdf", s.url) })).unwrap(),
            &m, &p, &ui,
        )
        .await;
        assert_eq!(js(&add)["ok"], true);
        assert_eq!(js(&add)["task_id"], m.list(None, None)[0].id);
        assert_eq!(p.calls.load(Ordering::SeqCst), 0, "manual adds are not confirmed again");

        let bad = handle(serde_json::from_value(json!({ "type": "add", "id": "b", "url": "ftp://x/y" })).unwrap(), &m, &p, &ui).await;
        assert_eq!(js(&bad), json!({ "id": "b", "ok": false, "error": "Tautan tidak valid." }));

        for (url, expected) in [("https://a.com/x.zip", Some("https://a.com/x.zip".to_string())), ("ftp://x", None)] {
            ui.0.lock().unwrap().clear();
            let show = handle(serde_json::from_value(json!({ "type": "show", "id": "s", "add_url": url })).unwrap(), &m, &p, &ui).await;
            assert_eq!(js(&show)["ok"], true);
            assert_eq!(*ui.0.lock().unwrap(), vec![expected]);
        }
    }

    #[tokio::test]
    async fn referrer_becomes_referer_header_only_when_missing() {
        let s = serve(body(300_000), ok()).await;
        let (_d, m) = setup();
        m.update_settings(Settings { confirm_dialog: false, ..m.settings() }).unwrap();
        let req: Request = serde_json::from_value(json!({ "type": "capture", "id": "1", "url": format!("{}/a.zip", s.url),
            "referrer": "https://page.test/dl", "headers": { "Cookie": "k=v" } }))
        .unwrap();
        handle(req, &m, &FakePrompter::default(), &FakeUi::default()).await;
        let h = s.last_headers();
        assert_eq!((h["referer"].as_str(), h["cookie"].as_str()), ("https://page.test/dl", "k=v"));
    }
}
