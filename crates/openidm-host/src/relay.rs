//! The relay loop, independent of real pipes and stdio so it can be tested.

use std::collections::HashSet;
use std::future::Future;
use std::io::{self, Read};
use std::time::{Duration, Instant};

use openidm_core::protocol::{read_frame, read_frame_async, request_id, write_frame_async};
use tokio::io::{AsyncRead, AsyncWrite, WriteHalf};
use tokio::sync::mpsc;

const POLL: Duration = Duration::from_millis(200);

/// How the host reaches the app. Faked in tests.
pub trait Connector {
    type Conn: AsyncRead + AsyncWrite + Send + 'static;
    fn connect(&mut self) -> impl Future<Output = io::Result<Self::Conn>>;
    /// Starts the app in the background. Called at most once per outage.
    fn launch_app(&mut self) -> io::Result<()>;
}

enum Event {
    Reply(u64, Vec<u8>),
    Closed(u64),
}

fn unavailable(id: &str) -> Vec<u8> {
    serde_json::json!({ "id": id, "ok": false, "error": "app_unavailable" }).to_string().into_bytes()
}

/// Reads browser frames from `reader` until end of input or a malformed frame, then closes `tx`.
pub fn pump_stdin(mut reader: impl Read, tx: mpsc::Sender<Vec<u8>>) {
    while let Ok(Some(frame)) = read_frame(&mut reader) {
        if tx.blocking_send(frame).is_err() {
            break;
        }
    }
}

/// Forwards browser frames to the app and app frames back to the browser until the browser closes.
///
/// When the app cannot be reached, or drops the connection with requests still unanswered, each
/// affected request is answered with `{"id", "ok": false, "error": "app_unavailable"}` so the
/// extension can hand the download back to the browser.
pub async fn relay<C, O>(
    mut incoming: mpsc::Receiver<Vec<u8>>,
    mut out: O,
    connector: &mut C,
    wait: Duration,
) -> io::Result<()>
where
    C: Connector,
    O: FnMut(&[u8]) -> io::Result<()>,
{
    let (events_tx, mut events) = mpsc::unbounded_channel::<Event>();
    let mut writer: Option<WriteHalf<C::Conn>> = None;
    let mut generation = 0u64;
    let mut pending: HashSet<String> = HashSet::new();
    // True after a launch that has not yet led to a working connection.
    let mut launched = false;

    loop {
        tokio::select! {
            frame = incoming.recv() => {
                let Some(frame) = frame else { return Ok(()) };
                let id = request_id(&frame);
                if writer.is_none() {
                    if let Some(conn) = connect(connector, wait, &mut launched).await {
                        generation += 1;
                        let (mut read, write) = tokio::io::split(conn);
                        let (tx, gen) = (events_tx.clone(), generation);
                        tokio::spawn(async move {
                            while let Ok(Some(reply)) = read_frame_async(&mut read).await {
                                if tx.send(Event::Reply(gen, reply)).is_err() {
                                    return;
                                }
                            }
                            let _ = tx.send(Event::Closed(gen));
                        });
                        writer = Some(write);
                    }
                }
                let sent = match writer.as_mut() {
                    Some(w) => write_frame_async(w, &frame).await.is_ok(),
                    None => false,
                };
                match (sent, id) {
                    (true, Some(id)) => { pending.insert(id); }
                    (true, None) => {}
                    (false, id) => {
                        writer = None;
                        if let Some(id) = id {
                            out(&unavailable(&id))?;
                        }
                    }
                }
            }
            Some(event) = events.recv() => match event {
                Event::Reply(gen, reply) if gen == generation => {
                    if let Some(id) = request_id(&reply) {
                        pending.remove(&id);
                    }
                    out(&reply)?;
                }
                Event::Closed(gen) if gen == generation => {
                    writer = None;
                    launched = false;
                    let mut ids: Vec<String> = pending.drain().collect();
                    ids.sort();
                    for id in ids {
                        out(&unavailable(&id))?;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Connects, launching the app and waiting up to `wait` for it the first time it is found missing.
async fn connect<C: Connector>(connector: &mut C, wait: Duration, launched: &mut bool) -> Option<C::Conn> {
    if let Ok(conn) = connector.connect().await {
        *launched = false;
        return Some(conn);
    }
    if *launched {
        // Already tried starting it during this outage; do not stall every later message.
        return None;
    }
    *launched = true;
    connector.launch_app().ok()?;
    let deadline = Instant::now() + wait;
    while Instant::now() + POLL <= deadline {
        tokio::time::sleep(POLL).await;
        if let Ok(conn) = connector.connect().await {
            *launched = false;
            return Some(conn);
        }
    }
    None
}
