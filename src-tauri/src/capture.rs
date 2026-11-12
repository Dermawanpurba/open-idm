//! Decides what happens to a request from the browser extension. No Tauri types here, so it is testable.

// IMPL

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
