use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use openidm_core::protocol::{read_frame_async, request_id, write_frame, write_frame_async};
use openidm_host::relay::{pump_stdin, relay, Connector};
use serde_json::{json, Value};
use tokio::io::DuplexStream;
use tokio::sync::mpsc;

/// What the fake app does with each connection.
#[derive(Clone, Copy)]
enum AppMode {
    /// Answer every request with `{"id", "ok": true}`.
    Echo,
    /// Read one request, then close without answering.
    DropAfterFirst,
}

struct FakeConnector {
    /// One entry per connect() call: `true` = succeed. When empty, connect fails.
    plan: VecDeque<bool>,
    mode: AppMode,
    launches: Arc<AtomicUsize>,
    connects: Arc<AtomicUsize>,
}

impl FakeConnector {
    fn new(plan: &[bool], mode: AppMode) -> Self {
        FakeConnector {
            plan: plan.iter().copied().collect(),
            mode,
            launches: Arc::new(AtomicUsize::new(0)),
            connects: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Connector for FakeConnector {
    type Conn = DuplexStream;

    async fn connect(&mut self) -> io::Result<DuplexStream> {
        self.connects.fetch_add(1, Ordering::SeqCst);
        if !self.plan.pop_front().unwrap_or(false) {
            return Err(io::ErrorKind::NotFound.into());
        }
        let (ours, mut app) = tokio::io::duplex(64 * 1024);
        let mode = self.mode;
        tokio::spawn(async move {
            while let Ok(Some(frame)) = read_frame_async(&mut app).await {
                match mode {
                    AppMode::DropAfterFirst => return,
                    AppMode::Echo => {
                        let reply = json!({ "id": request_id(&frame), "ok": true }).to_string();
                        if write_frame_async(&mut app, reply.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Ok(ours)
    }

    fn launch_app(&mut self) -> io::Result<()> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Feeds `messages` as browser frames (spaced by `gap`), runs the relay, returns what the browser would receive.
async fn run_relay(messages: &[&str], mut connector: FakeConnector, wait: Duration, gap: Duration) -> Vec<Value> {
    let (tx, rx) = mpsc::channel(8);
    let out = Arc::new(Mutex::new(Vec::<Value>::new()));
    let sink = out.clone();
    let owned: Vec<Vec<u8>> = messages.iter().map(|m| m.as_bytes().to_vec()).collect();
    tokio::spawn(async move {
        for m in owned {
            tx.send(m).await.unwrap();
            tokio::time::sleep(gap).await;
        }
        // Keep the "browser port" open long enough for replies, then close it.
        tokio::time::sleep(Duration::from_millis(900)).await;
    });
    relay(
        rx,
        move |frame: &[u8]| {
            sink.lock().unwrap().push(serde_json::from_slice(frame).unwrap());
            Ok(())
        },
        &mut connector,
        wait,
    )
    .await
    .unwrap();
    let got = out.lock().unwrap().clone();
    got
}

const QUICK: Duration = Duration::from_millis(20);

#[tokio::test]
async fn forwards_both_directions() {
    let c = FakeConnector::new(&[true], AppMode::Echo);
    let connects = c.connects.clone();
    let out = run_relay(&[r#"{"type":"ping","id":"1"}"#, r#"{"type":"ping","id":"2"}"#], c, Duration::from_secs(1), QUICK).await;
    assert_eq!(out, [json!({"id":"1","ok":true}), json!({"id":"2","ok":true})]);
    assert_eq!(connects.load(Ordering::SeqCst), 1, "one connection serves both requests");
}

#[tokio::test]
async fn launches_app_when_pipe_missing() {
    let c = FakeConnector::new(&[false, false, true], AppMode::Echo);
    let launches = c.launches.clone();
    let out = run_relay(&[r#"{"type":"ping","id":"1"}"#], c, Duration::from_secs(5), QUICK).await;
    assert_eq!(out, [json!({"id":"1","ok":true})]);
    assert_eq!(launches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn app_unavailable_after_wait() {
    let c = FakeConnector::new(&[], AppMode::Echo);
    let launches = c.launches.clone();
    let out = run_relay(&[r#"{"type":"ping","id":"a"}"#, r#"{"type":"ping","id":"b"}"#], c, Duration::from_millis(300), QUICK).await;
    assert_eq!(
        out,
        [json!({"id":"a","ok":false,"error":"app_unavailable"}), json!({"id":"b","ok":false,"error":"app_unavailable"})]
    );
    assert_eq!(launches.load(Ordering::SeqCst), 1, "the app is launched once, not once per message");
}

#[tokio::test]
async fn pipe_drop_fails_pending_ids() {
    let c = FakeConnector::new(&[true], AppMode::DropAfterFirst);
    let out = run_relay(&[r#"{"type":"capture","id":"c"}"#], c, Duration::from_millis(300), QUICK).await;
    assert_eq!(out, [json!({"id":"c","ok":false,"error":"app_unavailable"})]);
}

#[tokio::test]
async fn reconnects_and_relaunches_after_the_app_went_away() {
    // First connection drops the request; the app is then gone until relaunched.
    let c = FakeConnector::new(&[true, false, true], AppMode::DropAfterFirst);
    let launches = c.launches.clone();
    let connects = c.connects.clone();
    let out = run_relay(
        &[r#"{"type":"capture","id":"c"}"#, r#"{"type":"capture","id":"d"}"#],
        c,
        Duration::from_secs(2),
        Duration::from_millis(150),
    )
    .await;
    assert_eq!(out.len(), 2);
    assert_eq!(out[0]["id"], "c");
    assert_eq!(out[1]["id"], "d");
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn message_without_id_is_forwarded_and_not_tracked() {
    let c = FakeConnector::new(&[true], AppMode::Echo);
    let out = run_relay(&[r#"{"type":"ping"}"#], c, Duration::from_secs(1), QUICK).await;
    assert_eq!(out, [json!({"id":null,"ok":true})]);
}

#[test]
fn stdin_pump_stops_cleanly_on_oversized_or_garbage_input() {
    let (tx, mut rx) = mpsc::channel(8);
    let mut input = Vec::new();
    write_frame(&mut input, br#"{"id":"1"}"#).unwrap();
    input.extend_from_slice(&u32::MAX.to_le_bytes()); // declares a 4 GB frame
    input.extend_from_slice(b"garbage");
    pump_stdin(io::Cursor::new(input), tx);
    assert_eq!(rx.blocking_recv().unwrap(), br#"{"id":"1"}"#);
    assert!(rx.blocking_recv().is_none(), "channel closes instead of panicking");
}
