//! Browser <-> app protocol: Native Messaging framing and message types.
//! The same framing is used on the named pipe between the host and the app.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

pub const HOST_NAME: &str = "com.openidm.host";
/// Fixed by the public key in `browser-extension/manifest.json` (see scripts/gen-extension-key.cjs).
pub const EXTENSION_ID: &str = "nddkfocjigoaefookeikpnlkfebfclch";
/// Native Messaging caps host-to-browser messages at 1 MB; both directions use the same cap here.
pub const MAX_FRAME: usize = 1024 * 1024;

/// Reads one length-prefixed frame. `Ok(None)` means a clean end of stream.
pub fn read_frame<R: Read>(r: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    let mut filled = 0;
    while filled < 4 {
        match r.read(&mut len[filled..])? {
            0 if filled == 0 => return Ok(None),
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            n => filled += n,
        }
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(Some(payload))
}

pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

#[derive(Deserialize, Clone, Debug)]
pub struct CaptureItem {
    pub url: String,
    pub filename: Option<String>,
    #[serde(default)]
    pub filesize: u64,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub referrer: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Ping {
        id: String,
    },
    Capture {
        id: String,
        #[serde(flatten)]
        item: CaptureItem,
    },
    Add {
        id: String,
        #[serde(flatten)]
        item: CaptureItem,
    },
    Show {
        id: String,
        add_url: Option<String>,
    },
}

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Accepted,
    Later,
    Declined,
    Failed,
}

#[derive(Serialize, Default, Debug)]
pub struct Response {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

impl Response {
    pub fn error(id: impl Into<String>, message: impl Into<String>) -> Response {
        Response { id: id.into(), ok: false, error: Some(message.into()), ..Default::default() }
    }
}

/// Pulls the `id` out of any JSON message, even one that is not a valid `Request`.
pub fn request_id(raw: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    value.get("id")?.as_str().map(str::to_owned)
}

/// Keeps only the request headers a download needs, under canonical names.
pub fn filter_headers(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let canonical = match name.to_ascii_lowercase().as_str() {
                "cookie" => "Cookie",
                "referer" => "Referer",
                "user-agent" => "User-Agent",
                "origin" => "Origin",
                _ => return None,
            };
            let clean = !value.contains(['\r', '\n']) && !value.is_empty();
            clean.then(|| (canonical.to_string(), value.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"{\"a\":1}").unwrap();
        assert_eq!(&buf[..4], &7u32.to_le_bytes());
        let mut c = std::io::Cursor::new(buf);
        assert_eq!(read_frame(&mut c).unwrap().unwrap(), b"{\"a\":1}");
        assert!(read_frame(&mut c).unwrap().is_none());
    }

    #[test]
    fn frame_too_large_rejected() {
        let mut c = std::io::Cursor::new(((MAX_FRAME as u32) + 1).to_le_bytes().to_vec());
        assert_eq!(read_frame(&mut c).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn frame_truncated_is_error() {
        let mut c = std::io::Cursor::new(vec![5, 0, 0, 0, b'a']);
        assert!(read_frame(&mut c).is_err());
    }

    #[test]
    fn parse_capture() {
        let r: Request = serde_json::from_str(
            r#"{"type":"capture","id":"7","url":"https://a.com/f.zip","filesize":10,"headers":{"Cookie":"x"}}"#,
        )
        .unwrap();
        match r {
            Request::Capture { id, item } => {
                assert_eq!(id, "7");
                assert_eq!(item.filesize, 10);
                assert!(item.filename.is_none());
            }
            _ => panic!("expected capture"),
        }
    }

    #[test]
    fn unknown_type_is_error_but_id_recoverable() {
        let raw = br#"{"type":"nope","id":"9"}"#;
        assert!(serde_json::from_slice::<Request>(raw).is_err());
        assert_eq!(request_id(raw).as_deref(), Some("9"));
    }

    #[test]
    fn response_shape() {
        let s = serde_json::to_string(&Response {
            id: "1".into(),
            ok: true,
            decision: Some(Decision::Accepted),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(s, r#"{"id":"1","ok":true,"decision":"accepted"}"#);
    }

    #[test]
    fn header_allowlist() {
        let h: BTreeMap<_, _> = [
            ("Cookie", "a"), ("referer", "b"), ("Authorization", "c"), ("X-Foo", "d"), ("User-Agent", "e"),
            ("Origin", "f"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let f = filter_headers(&h);
        assert_eq!(f.keys().cloned().collect::<Vec<_>>(), ["Cookie", "Origin", "Referer", "User-Agent"]);
    }

    #[test]
    fn header_with_line_break_dropped() {
        let h: BTreeMap<_, _> = [("Cookie".to_string(), "a\r\nX-Injected: 1".to_string())].into_iter().collect();
        assert!(filter_headers(&h).is_empty());
    }

    #[test]
    fn pipe_name_has_sid() {
        let n = crate::winutil::pipe_name().unwrap();
        assert!(n.starts_with(r"\\.\pipe\openidm-S-1-"), "{n}");
    }
}
