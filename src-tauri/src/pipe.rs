//! Named-pipe server the Native Messaging host connects to. Frames use the browser's wire format.

use std::future::Future;
use std::io;

use openidm_core::protocol::{read_frame_async, write_frame_async};
use openidm_core::winutil::UserOnlySecurity;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Creates one pipe instance that only the current user can open and no remote client can reach.
fn make_server(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let mut security = UserOnlySecurity::new()?;
    let mut options = ServerOptions::new();
    // `first` fails if another process already owns the name, so nothing can impersonate the app.
    options.first_pipe_instance(first).reject_remote_clients(true);
    // SAFETY: `security` outlives the call and points at a valid SECURITY_ATTRIBUTES.
    unsafe { options.create_with_security_attributes_raw(name, security.as_ptr()) }
}

/// Serves `name` until an error. Every frame is handled on its own task, so a request that waits
/// (a confirmation dialog) never blocks the others. `handler` gets a token that is cancelled when
/// its connection closes.
pub async fn serve_on<H, Fut>(name: String, handler: H) -> io::Result<()>
where
    H: Fn(Vec<u8>, CancellationToken) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Option<Vec<u8>>> + Send + 'static,
{
    let mut server = make_server(&name, true)?;
    loop {
        server.connect().await?;
        let connected = std::mem::replace(&mut server, make_server(&name, false)?);
        tokio::spawn(connection(connected, handler.clone()));
    }
}

async fn connection<H, Fut>(pipe: NamedPipeServer, handler: H)
where
    H: Fn(Vec<u8>, CancellationToken) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Option<Vec<u8>>> + Send + 'static,
{
    let closed = CancellationToken::new();
    let (mut read, mut write) = tokio::io::split(pipe);
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if write_frame_async(&mut write, &frame).await.is_err() {
                break;
            }
        }
    });
    while let Ok(Some(frame)) = read_frame_async(&mut read).await {
        let (handler, tx, closed) = (handler.clone(), tx.clone(), closed.clone());
        tokio::spawn(async move {
            if let Some(reply) = handler(frame, closed).await {
                let _ = tx.send(reply);
            }
        });
    }
    // The browser side is gone: anything still waiting on the user must stop.
    closed.cancel();
    drop(tx);
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::net::windows::named_pipe::ClientOptions;

    use super::*;

    fn unique_name(tag: &str) -> String {
        format!(r"\\.\pipe\openidm-test-{tag}-{}", std::process::id())
    }

    async fn client(name: &str) -> tokio::net::windows::named_pipe::NamedPipeClient {
        for _ in 0..100 {
            if let Ok(c) = ClientOptions::new().open(name) {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("pipe never became available");
    }

    #[tokio::test]
    async fn a_slow_request_does_not_block_a_fast_one() {
        let name = unique_name("order");
        tokio::spawn(serve_on(name.clone(), |frame: Vec<u8>, _closed| async move {
            if frame == b"slow" {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            Some([b"re:", frame.as_slice()].concat())
        }));
        let mut c = client(&name).await;
        write_frame_async(&mut c, b"slow").await.unwrap();
        write_frame_async(&mut c, b"fast").await.unwrap();
        assert_eq!(read_frame_async(&mut c).await.unwrap().unwrap(), b"re:fast");
        assert_eq!(read_frame_async(&mut c).await.unwrap().unwrap(), b"re:slow");
    }

    #[tokio::test]
    async fn serves_several_clients_and_survives_a_disconnect() {
        let name = unique_name("multi");
        tokio::spawn(serve_on(name.clone(), |frame: Vec<u8>, _closed| async move { Some(frame) }));
        let mut a = client(&name).await;
        write_frame_async(&mut a, b"one").await.unwrap();
        assert_eq!(read_frame_async(&mut a).await.unwrap().unwrap(), b"one");
        drop(a);
        let mut b = client(&name).await;
        write_frame_async(&mut b, b"two").await.unwrap();
        assert_eq!(read_frame_async(&mut b).await.unwrap().unwrap(), b"two");
    }

    #[tokio::test]
    async fn closing_the_connection_cancels_waiting_requests() {
        let name = unique_name("cancel");
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel();
        tokio::spawn(serve_on(name.clone(), move |_frame: Vec<u8>, closed: CancellationToken| {
            let seen_tx = seen_tx.clone();
            async move {
                closed.cancelled().await;
                let _ = seen_tx.send(());
                None
            }
        }));
        let mut c = client(&name).await;
        write_frame_async(&mut c, b"wait").await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(c);
        tokio::time::timeout(Duration::from_secs(2), seen_rx.recv()).await.expect("handler was cancelled").unwrap();
    }

    #[tokio::test]
    async fn a_second_owner_of_the_name_is_refused() {
        let name = unique_name("owner");
        let _first = make_server(&name, true).unwrap();
        assert!(make_server(&name, true).is_err());
    }
}
