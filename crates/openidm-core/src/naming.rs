//! URL validation, filename sanitisation, categories and collision-free paths.

use std::path::{Component, Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

const MAX_NAME_CHARS: usize = 180;

/// Accepts only http/https URLs without embedded credentials.
pub fn http_url(input: &str) -> Option<Url> {
    let url = Url::parse(input.trim()).ok()?;
    let ok = matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some();
    ok.then_some(url)
}

fn trim_trailing(name: &str) -> &str {
    name.trim_end_matches(['.', ' '])
}

/// Reduces any input to a single safe Windows filename. May return an empty string.
pub fn safe_filename(input: &str) -> String {
    let last = input.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') && !c.is_control())
        .collect();
    let trimmed = trim_trailing(cleaned.trim_start_matches('.')).trim();
    let cut: String = trimmed.chars().take(MAX_NAME_CHARS).collect();
    let mut name = trim_trailing(&cut).to_string();

    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if reserved {
        name.insert(0, '_');
    }
    name
}

/// Extracts the filename from a Content-Disposition header (RFC 6266 / RFC 5987).
pub fn filename_from_disposition(header: &str) -> Option<String> {
    let mut plain = None;
    for part in header.split(';').skip(1) {
        let Some((key, value)) = part.split_once('=') else { continue };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "filename*" => {
                let encoded = value.splitn(3, '\'').nth(2).unwrap_or(value);
                let decoded = percent_decode_str(encoded).decode_utf8_lossy().into_owned();
                if !decoded.is_empty() {
                    return Some(decoded);
                }
            }
            "filename" => {
                let unquoted = value.trim_matches('"');
                if !unquoted.is_empty() {
                    plain = Some(unquoted.to_string());
                }
            }
            _ => {}
        }
    }
    plain
}

/// Picks the filename: caller-supplied, then server header, then URL path, then "download".
pub fn resolve_filename(given: Option<&str>, disposition: Option<&str>, url: &Url) -> String {
    let from_url = url
        .path_segments()
        .and_then(|mut s| s.next_back().map(str::to_owned))
        .map(|s| percent_decode_str(&s).decode_utf8_lossy().into_owned());
    let candidates = [
        given.map(str::to_owned),
        disposition.and_then(filename_from_disposition),
        from_url,
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|c| safe_filename(&c))
        .find(|c| !c.is_empty())
        .unwrap_or_else(|| "download".to_string())
}

pub fn category_for(filename: &str) -> &'static str {
    let ext = match filename.rsplit_once('.') {
        Some((_, e)) => e.to_ascii_lowercase(),
        None => return "General",
    };
    match ext.as_str() {
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "flv" | "wmv" | "mpg" | "mpeg" | "3gp" => "Video",
        "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "opus" | "wma" => "Music",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "csv" | "rtf" | "odt" | "ods"
        | "epub" => "Documents",
        "exe" | "msi" | "apk" | "dmg" | "deb" | "rpm" | "appx" | "msix" | "bat" => "Programs",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "iso" | "zst" => "Compressed",
        _ => "General",
    }
}

pub const CATEGORIES: [&str; 6] = ["Video", "Music", "Documents", "Programs", "Compressed", "General"];

/// Maps untrusted input to a known category; anything else is rejected.
pub fn valid_category(input: Option<&str>) -> Option<&'static str> {
    let input = input?;
    CATEGORIES.iter().copied().find(|c| *c == input)
}

/// Returns `dir/filename`, or `dir/stem (n).ext` when the file or its `.part` already exists.
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let taken = |p: &Path| {
        let mut part = p.as_os_str().to_owned();
        part.push(".part");
        p.exists() || Path::new(&part).exists()
    };
    let first = dir.join(filename);
    if !taken(&first) {
        return first;
    }
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (filename, String::new()),
    };
    (1u32..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !taken(p))
        .expect("an unused name exists")
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Lexical containment check: true when `candidate` resolves to a path under `dir`.
pub fn is_inside(dir: &Path, candidate: &Path) -> bool {
    let dir = normalize(dir);
    let candidate = normalize(candidate);
    candidate != dir && candidate.starts_with(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_rules() {
        assert!(http_url("https://a.com/f.zip").is_some());
        assert!(http_url("ftp://a.com/f").is_none());
        assert!(http_url("https://user:pw@a.com/f").is_none());
        assert!(http_url("javascript:alert(1)").is_none());
    }

    #[test]
    fn filename_sanitized() {
        assert_eq!(safe_filename("../../etc/pass:wd?.txt"), "passwd.txt");
        assert_eq!(safe_filename("C:\\x\\y\\report.pdf"), "report.pdf");
        assert_eq!(safe_filename("CON.txt"), "_CON.txt");
        assert_eq!(safe_filename("name. . "), "name");
        assert_eq!(safe_filename(&"a".repeat(300)).chars().count(), 180);
        assert_eq!(safe_filename("..."), "");
    }

    #[test]
    fn disposition_variants() {
        assert_eq!(
            filename_from_disposition("attachment; filename*=UTF-8''Laporan%20%C3%A9.zip").unwrap(),
            "Laporan é.zip"
        );
        assert_eq!(filename_from_disposition("attachment; filename=\"a b.zip\"").unwrap(), "a b.zip");
        assert_eq!(filename_from_disposition("attachment; filename=plain.bin").unwrap(), "plain.bin");
        assert!(filename_from_disposition("inline").is_none());
    }

    #[test]
    fn resolve_priority_and_fallback() {
        let u = http_url("https://a.com/dl/Motorgrader%20Group.zip?x=1").unwrap();
        assert_eq!(resolve_filename(Some("mine.zip"), Some("attachment; filename=srv.zip"), &u), "mine.zip");
        assert_eq!(resolve_filename(None, Some("attachment; filename=srv.zip"), &u), "srv.zip");
        assert_eq!(resolve_filename(None, None, &u), "Motorgrader Group.zip");
        assert_eq!(resolve_filename(Some("..."), None, &http_url("https://a.com/").unwrap()), "download");
    }

    #[test]
    fn categories() {
        for (f, c) in [
            ("a.mp4", "Video"), ("a.mkv", "Video"), ("a.mp3", "Music"), ("a.flac", "Music"),
            ("a.pdf", "Documents"), ("a.xlsx", "Documents"), ("a.exe", "Programs"), ("a.msi", "Programs"),
            ("a.zip", "Compressed"), ("a.7z", "Compressed"), ("a.iso", "Compressed"), ("a", "General"),
            ("a.xyz", "General"),
        ] {
            assert_eq!(category_for(f), c, "{f}");
        }
    }

    #[test]
    fn unique_and_inside() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(unique_path(d.path(), "a.zip"), d.path().join("a.zip"));
        std::fs::write(d.path().join("a.zip"), b"x").unwrap();
        std::fs::write(d.path().join("a (1).zip.part"), b"x").unwrap();
        assert_eq!(unique_path(d.path(), "a.zip"), d.path().join("a (2).zip"));
        assert!(is_inside(d.path(), &d.path().join("Video").join("a.mp4")));
        assert!(!is_inside(d.path(), &d.path().join("..").join("a.mp4")));
    }
}
