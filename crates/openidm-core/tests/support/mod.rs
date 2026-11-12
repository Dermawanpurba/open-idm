//! Programmable local HTTP file server for engine and manager tests.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::Router;
use bytes::Bytes;

#[derive(Clone, Default)]
pub struct ServeOpts {
    /// Honour `Range` requests with 206.
    pub ranges: bool,
    /// Send a `Content-Length` header.
    pub content_length: bool,
    /// Answer every request with this status and no body.
    pub status: Option<u16>,
    /// Abort each response after this many body bytes.
    pub drop_after: Option<usize>,
    pub disposition: Option<String>,
    /// Answer range requests starting at or beyond the offset with the given status.
    pub status_on_range_from: Option<(u64, u16)>,
    /// Limit each response to this many bytes per second.
    pub throttle_bps: Option<u64>,
    /// Honour only the `bytes=0-0` probe; answer any other range with a full 200 body.
    pub ignore_range_after_probe: bool,
}

/// A well-behaved server: ranges and content length.
pub fn ok() -> ServeOpts {
    ServeOpts { ranges: true, content_length: true, ..Default::default() }
}

/// Deterministic pseudo-random bytes.
pub fn body(n: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

struct Shared {
    body: Bytes,
    opts: Mutex<ServeOpts>,
    hits: Arc<AtomicUsize>,
    served: Arc<AtomicU64>,
    last_headers: Mutex<HashMap<String, String>>,
}

pub struct TestServer {
    pub url: String,
    pub hits: Arc<AtomicUsize>,
    shared: Arc<Shared>,
}

impl TestServer {
    pub fn bytes_served(&self) -> u64 {
        self.shared.served.load(Ordering::SeqCst)
    }
    pub fn set_opts(&self, opts: ServeOpts) {
        *self.shared.opts.lock().unwrap() = opts;
    }
    pub fn last_headers(&self) -> HashMap<String, String> {
        self.shared.last_headers.lock().unwrap().clone()
    }
}

pub async fn serve(body: Vec<u8>, opts: ServeOpts) -> TestServer {
    let hits = Arc::new(AtomicUsize::new(0));
    let shared = Arc::new(Shared {
        body: Bytes::from(body),
        opts: Mutex::new(opts),
        hits: hits.clone(),
        served: Arc::new(AtomicU64::new(0)),
        last_headers: Mutex::new(HashMap::new()),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(handle).with_state(shared.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    TestServer { url, hits, shared }
}

fn parse_range(headers: &HeaderMap) -> Option<(u64, Option<u64>)> {
    let value = headers.get(header::RANGE)?.to_str().ok()?.strip_prefix("bytes=")?;
    let (a, b) = value.split_once('-')?;
    Some((a.parse().ok()?, b.parse().ok()))
}

fn empty(status: u16) -> Response {
    Response::builder().status(StatusCode::from_u16(status).unwrap()).body(Body::empty()).unwrap()
}

async fn handle(State(s): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    s.hits.fetch_add(1, Ordering::SeqCst);
    *s.last_headers.lock().unwrap() = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let o = s.opts.lock().unwrap().clone();
    if let Some(status) = o.status {
        return empty(status);
    }
    let total = s.body.len() as u64;
    let range = parse_range(&headers);
    let is_probe = range == Some((0, Some(0)));
    let honour = o.ranges && total > 0 && !(o.ignore_range_after_probe && !is_probe);
    let (start, end, status) = match range {
        Some((a, b)) if honour => {
            if let Some((from, st)) = o.status_on_range_from {
                if a >= from {
                    return empty(st);
                }
            }
            (a, b.unwrap_or(total - 1).min(total - 1), 206)
        }
        _ => (0, total.saturating_sub(1), 200),
    };
    let slice = if total == 0 { Bytes::new() } else { s.body.slice(start as usize..=end as usize) };
    let len = slice.len();

    let mut builder = Response::builder().status(status);
    if status == 206 {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"));
    }
    if o.ranges {
        builder = builder.header(header::ACCEPT_RANGES, "bytes");
    }
    if o.content_length {
        builder = builder.header(header::CONTENT_LENGTH, len);
    }
    if let Some(d) = &o.disposition {
        builder = builder.header(header::CONTENT_DISPOSITION, d);
    }

    let served = s.served.clone();
    let stream = futures_util::stream::unfold((0usize, false), move |(pos, failed)| {
        let slice = slice.clone();
        let served = served.clone();
        let o = o.clone();
        async move {
            if failed || pos >= len {
                return None;
            }
            if let Some(limit) = o.drop_after {
                if pos >= limit {
                    let err = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "dropped by test server");
                    return Some((Err(err), (pos, true)));
                }
            }
            let mut n = (len - pos).min(16 * 1024);
            if let Some(limit) = o.drop_after {
                n = n.min(limit - pos);
            }
            if let Some(bps) = o.throttle_bps {
                tokio::time::sleep(Duration::from_secs_f64(n as f64 / bps as f64)).await;
            }
            served.fetch_add(n as u64, Ordering::SeqCst);
            Some((Ok::<_, std::io::Error>(slice.slice(pos..pos + n)), (pos + n, false)))
        }
    });
    builder.body(Body::from_stream(stream)).unwrap()
}
