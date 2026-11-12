# OpenIDM Rust Tahap 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Aplikasi desktop OpenIDM berbasis Rust/Tauri yang otomatis mengambil alih unduhan file dari Chrome/Edge lewat Native Messaging, menggantikan dashboard tab `127.0.0.1:6899` untuk file langsung.

**Architecture:** Cargo workspace dengan tiga crate: `openidm-core` (engine unduh, antrean, SQLite, protokol; tanpa UI), `openidm-host` (relay Native Messaging stdin/stdout ↔ named pipe), dan `src-tauri` (jendela, tray, dialog konfirmasi, server pipe). UI HTML/JS yang ada dipakai ulang lewat adapter `invoke`. Core Python lama dipindah ke subfolder `legacy\` tanpa diubah.

**Tech Stack:** Rust stable `x86_64-pc-windows-gnu`, MinGW-w64 (WinLibs UCRT), Tauri 2, tokio, reqwest (rustls), rusqlite (bundled), serde, windows-sys, fs4; dev: axum, tempfile. Extension: Manifest V3, `node --test`.

**Spec:** `docs/superpowers/specs/2026-11-12-openidm-rust-tahap1-design.md`

## Global Constraints

- Semua toolchain dan cache build di `D:\rust` (`RUSTUP_HOME=D:\rust\rustup`, `CARGO_HOME=D:\rust\cargo`, target dir `D:/rust/target/openidm`, MinGW di `D:\rust\mingw64`). Jangan memasang apa pun yang besar ke C: (sisa 4,8 GB).
- Tidak ada OpenSSL: reqwest dengan `default-features = false, features = ["rustls-tls", "stream"]`.
- Kode di `core/*.py`, `main.py`, `OpenIDM.spec` tidak diubah.
- App Rust tidak membuka port HTTP apa pun.
- Nama host Native Messaging: `com.openidm.host`. Nama pipe: `\\.\pipe\openidm-<SID pengguna>`.
- Nilai default: `segments = 8`, `max_concurrent = 3`, ambang multi-segmen 2 MB (`2 * 1024 * 1024`), cadangan disk 100 MB, coba ulang segmen 5 kali, host menunggu app paling lama 5 detik, nama file ≤ 180 karakter.
- Header yang boleh diteruskan: `Cookie`, `Referer`, `User-Agent`, `Origin`.
- Kategori: `Video`, `Music`, `Documents`, `Programs`, `Compressed`, `General`.
- Teks UI dalam Bahasa Indonesia. Ikon SVG, tanpa emoji, tanpa gradien ungu/pink; setiap kontrol punya state hover/active/focus-visible/disabled.
- Pesan error ke UI/extension tanpa stack trace atau path internal.
- Sebelum memakai API Tauri 2 atau plugin-nya, cek dokumentasi terkini lewat MCP `context7`; jangan menulis dari ingatan.
- Proyek belum berupa repo git. Task 1 menjalankan `git init` lokal (tanpa remote) agar tiap task bisa di-commit.

## Review Focus

1. **Tautan bertanda tangan kedaluwarsa saat dilanjutkan** (403/410 pada resume, umum di Drive/Takeout): tugas menjadi `error` dengan pesan "Tautan sudah kedaluwarsa. Unduh ulang dari website." dan file `.part` tidak dihapus. → test di Task 6.
2. **Dua unduhan diklik hampir bersamaan**: muncul dua dialog terpisah, masing-masing dijawab sendiri, jawaban tidak tertukar. → test di Task 10.
3. **App mati saat dialog terbuka atau sebelum byte pertama**: pipe putus → host mengirim `{"ok":false,"error":"app_unavailable"}` untuk setiap `id` yang belum dibalas → extension melanjutkan unduhan browser. → test di Task 8 dan Task 11.
4. **Nama file dari server**: `Content-Disposition` RFC 5987 (`filename*=UTF-8''...`), nama Unicode, tanpa ekstensi, atau kosong → nama aman, tidak kosong (`download`), ekstensi dari URL bila ada. → test di Task 2.
5. **Folder unduhan tidak tersedia** (drive dicabut, folder dihapus): tugas `error` dengan pesan "Folder unduhan tidak bisa diakses: <path>", app tidak crash. → test di Task 7.

---

## File Structure

```
open-idm-src/
├── Cargo.toml                      workspace
├── .cargo/config.toml              target-dir
├── .gitignore
├── build-rust.bat
├── crates/openidm-core/
│   ├── Cargo.toml
│   ├── src/lib.rs                  re-export modul
│   ├── src/naming.rs               URL, nama file, kategori, path unik
│   ├── src/protocol.rs             framing + tipe pesan + konstanta
│   ├── src/db.rs                   SQLite
│   ├── src/settings.rs             settings.json
│   ├── src/engine.rs               satu unduhan: probe, segmen, resume
│   ├── src/manager.rs              antrean dan siklus hidup tugas
│   ├── src/winutil.rs              SID pengguna, nama pipe
│   └── tests/{support/mod.rs,engine.rs,manager.rs}
├── crates/openidm-host/
│   ├── Cargo.toml
│   ├── src/main.rs                 relay
│   └── tests/relay.rs
├── src-tauri/
│   ├── Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json
│   └── src/{main.rs,commands.rs,pipe.rs,capture.rs,register.rs,tray.rs}
├── ui/{api.js,confirm.html,confirm.js}        baru
├── ui/{index.html,app.js,styles.css}          diubah
├── browser-extension/{native.js}              baru
├── browser-extension/{manifest.json,background.js,capture.js,popup.js,INSTALL.md}  diubah
├── scripts/gen-extension-key.cjs
└── tests/extension.test.cjs                   diubah
```

---

### Task 1: Toolchain di D: dan kerangka workspace

**Files:**
- Create: `Cargo.toml`, `.cargo/config.toml`, `.gitignore`, `crates/openidm-core/Cargo.toml`, `crates/openidm-core/src/lib.rs`

**Interfaces:**
- Produces: perintah `cargo test -p openidm-core` yang berjalan; setiap task berikutnya memakai shell dengan `RUSTUP_HOME`, `CARGO_HOME` terpasang dan `D:\rust\cargo\bin;D:\rust\mingw64\bin` di PATH.

- [ ] **Step 1:** Buat `D:\rust`. Set variabel lingkungan tingkat User: `RUSTUP_HOME=D:\rust\rustup`, `CARGO_HOME=D:\rust\cargo`; tambahkan `D:\rust\cargo\bin` dan `D:\rust\mingw64\bin` ke PATH User.
- [ ] **Step 2:** Unduh `rustup-init.exe` (https://win.rustup.rs/x86_64) ke `D:\rust`, jalankan `rustup-init.exe -y --default-host x86_64-pc-windows-gnu --default-toolchain stable --profile minimal --no-modify-path`.
- [ ] **Step 3:** Unduh rilis WinLibs terbaru "UCRT, POSIX threads, x86_64, tanpa LLVM" (zip) dari https://github.com/brechtsanders/winlibs_mingw/releases dan ekstrak sehingga `D:\rust\mingw64\bin\gcc.exe` ada.
- [ ] **Step 4:** Verifikasi: `rustc -vV` menampilkan `host: x86_64-pc-windows-gnu`; `gcc --version` dan `windres --version` berjalan; `(Get-PSDrive C).Free` tidak turun lebih dari 200 MB dibanding sebelum Step 1.
- [ ] **Step 5:** Tulis `Cargo.toml` workspace (`members = ["crates/openidm-core", "crates/openidm-host", "src-tauri"]`, `resolver = "2"`, `[profile.release] opt-level = "s", lto = true, strip = true`). Untuk sekarang hanya daftarkan `crates/openidm-core`; anggota lain ditambahkan di task-nya. `.cargo/config.toml`: `[build] target-dir = "D:/rust/target/openidm"`.
- [ ] **Step 6:** `crates/openidm-core` dengan `lib.rs` berisi satu test `workspace_builds()` yang `assert_eq!(2 + 2, 4)`. Run: `cargo test -p openidm-core`. Expected: `1 passed`.
- [ ] **Step 7:** `.gitignore`: `/bin/`, `/build/`, `/dist/`, `__pycache__/`, `*.pyc`, `/target/`, `*.db`, `*.db-*`, `/scripts/icon-cache/`. Lalu `git init`, `git add -A`, `git commit -m "chore: baseline sebelum migrasi Rust + kerangka workspace"`.

---

### Task 2: `naming.rs` — URL, nama file, kategori

**Files:**
- Create: `crates/openidm-core/src/naming.rs`
- Modify: `crates/openidm-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub fn http_url(input: &str) -> Option<url::Url>` — hanya `http`/`https`, tanpa username/password.
  - `pub fn safe_filename(input: &str) -> String` — hasil boleh kosong.
  - `pub fn filename_from_disposition(header: &str) -> Option<String>`
  - `pub fn resolve_filename(given: Option<&str>, disposition: Option<&str>, url: &url::Url) -> String` — urutan prioritas: `given`, `disposition`, segmen terakhir path URL (percent-decoded), lalu `"download"`.
  - `pub fn category_for(filename: &str) -> &'static str`
  - `pub fn unique_path(dir: &Path, filename: &str) -> PathBuf` — `nama (1).ext`, `nama (2).ext`, … bila file atau `<file>.part` sudah ada.
  - `pub fn is_inside(dir: &Path, candidate: &Path) -> bool`

- [ ] **Step 1: Tulis test yang gagal** (modul `#[cfg(test)]` di `naming.rs`):

```rust
#[test] fn url_rules() {
    assert!(http_url("https://a.com/f.zip").is_some());
    assert!(http_url("ftp://a.com/f").is_none());
    assert!(http_url("https://user:pw@a.com/f").is_none());
    assert!(http_url("javascript:alert(1)").is_none());
}
#[test] fn filename_sanitized() {
    assert_eq!(safe_filename("../../etc/pass:wd?.txt"), "passwd.txt");
    assert_eq!(safe_filename("C:\\x\\y\\report.pdf"), "report.pdf");
    assert_eq!(safe_filename("CON.txt"), "_CON.txt");
    assert_eq!(safe_filename("name. . "), "name");
    assert_eq!(safe_filename(&"a".repeat(300)).chars().count(), 180);
    assert_eq!(safe_filename("..."), "");
}
#[test] fn disposition_variants() {
    assert_eq!(filename_from_disposition("attachment; filename*=UTF-8''Laporan%20%C3%A9.zip").unwrap(), "Laporan é.zip");
    assert_eq!(filename_from_disposition("attachment; filename=\"a b.zip\"").unwrap(), "a b.zip");
    assert_eq!(filename_from_disposition("attachment; filename=plain.bin").unwrap(), "plain.bin");
    assert!(filename_from_disposition("inline").is_none());
}
#[test] fn resolve_priority_and_fallback() {
    let u = http_url("https://a.com/dl/Motorgrader%20Group.zip?x=1").unwrap();
    assert_eq!(resolve_filename(Some("mine.zip"), Some("attachment; filename=srv.zip"), &u), "mine.zip");
    assert_eq!(resolve_filename(None, Some("attachment; filename=srv.zip"), &u), "srv.zip");
    assert_eq!(resolve_filename(None, None, &u), "Motorgrader Group.zip");
    assert_eq!(resolve_filename(Some("..."), None, &http_url("https://a.com/").unwrap()), "download");
}
#[test] fn categories() {
    for (f, c) in [("a.mp4","Video"),("a.mkv","Video"),("a.mp3","Music"),("a.flac","Music"),
                   ("a.pdf","Documents"),("a.xlsx","Documents"),("a.exe","Programs"),("a.msi","Programs"),
                   ("a.zip","Compressed"),("a.7z","Compressed"),("a.iso","Compressed"),("a","General"),("a.xyz","General")] {
        assert_eq!(category_for(f), c, "{f}");
    }
}
#[test] fn unique_and_inside() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(unique_path(d.path(), "a.zip"), d.path().join("a.zip"));
    std::fs::write(d.path().join("a.zip"), b"x").unwrap();
    std::fs::write(d.path().join("a (1).zip.part"), b"x").unwrap();
    assert_eq!(unique_path(d.path(), "a.zip"), d.path().join("a (2).zip"));
    assert!(is_inside(d.path(), &d.path().join("Video").join("a.mp4")));
    assert!(!is_inside(d.path(), &d.path().join("..").join("a.mp4")));
}
```

- [ ] **Step 2:** Run `cargo test -p openidm-core naming`. Expected: gagal kompilasi (fungsi belum ada).
- [ ] **Step 3:** Implementasikan fungsi di Interfaces. Aturan `safe_filename` sama dengan `browser-extension/shared.js:34-39` (ambil komponen terakhir, buang `<>:"|?*` dan karakter kontrol, buang titik di depan dan titik/spasi di belakang, potong 180, awali `_` untuk nama perangkat). Dependensi: `url`, `percent-encoding`; dev: `tempfile`.
- [ ] **Step 4:** Run `cargo test -p openidm-core naming`. Expected: 6 passed.
- [ ] **Step 5:** `git commit -am "feat(core): aturan nama file, URL, dan kategori"`.

---

### Task 3: `protocol.rs` dan `winutil.rs`

**Files:**
- Create: `crates/openidm-core/src/protocol.rs`, `crates/openidm-core/src/winutil.rs`, `scripts/gen-extension-key.cjs`

**Interfaces:**
- Produces:
  - `pub const HOST_NAME: &str = "com.openidm.host";`
  - `pub const EXTENSION_ID: &str` — hasil Step 1.
  - `pub const MAX_FRAME: usize = 1024 * 1024;`
  - `pub fn read_frame<R: Read>(r: &mut R) -> io::Result<Option<Vec<u8>>>` — `Ok(None)` pada EOF bersih; `InvalidData` bila panjang > `MAX_FRAME`.
  - `pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> io::Result<()>` — 4 byte panjang little-endian + payload, lalu flush.
  - ```rust
    #[derive(Deserialize)] #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Request {
        Ping { id: String },
        Capture { id: String, #[serde(flatten)] item: CaptureItem },
        Add { id: String, #[serde(flatten)] item: CaptureItem },
        Show { id: String, add_url: Option<String> },
    }
    #[derive(Deserialize, Clone)]
    pub struct CaptureItem { pub url: String, pub filename: Option<String>, #[serde(default)] pub filesize: u64,
        #[serde(default)] pub headers: BTreeMap<String, String>, pub referrer: Option<String> }
    #[derive(Serialize)] #[serde(rename_all = "snake_case")]
    pub enum Decision { Accepted, Later, Declined, Failed }
    #[derive(Serialize, Default)]
    pub struct Response { pub id: String, pub ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")] pub decision: Option<Decision>,
        #[serde(skip_serializing_if = "Option::is_none")] pub task_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")] pub version: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")] pub capabilities: Vec<String> }
    pub fn request_id(raw: &[u8]) -> Option<String>   // ambil "id" dari JSON apa pun
    pub fn filter_headers(h: &BTreeMap<String,String>) -> BTreeMap<String,String>
    ```
  - `winutil::current_user_sid() -> io::Result<String>`, `winutil::pipe_name() -> io::Result<String>` (`\\.\pipe\openidm-<SID>`).

- [ ] **Step 1:** Tulis `scripts/gen-extension-key.cjs`: buat pasangan kunci RSA 2048 (`crypto.generateKeyPairSync`), cetak kunci publik DER base64 (untuk `manifest.key`) dan ID extension (SHA-256 dari DER, 16 byte pertama, tiap nibble dipetakan `0-f` → `a-p`). Kunci privat tidak disimpan. Jalankan sekali: `node scripts/gen-extension-key.cjs`; catat kedua nilai. Masukkan ID ke `EXTENSION_ID`; nilai `key` dipakai di Task 11.
- [ ] **Step 2: Tulis test yang gagal:**

```rust
#[test] fn frame_roundtrip() {
    let mut buf = Vec::new(); write_frame(&mut buf, b"{\"a\":1}").unwrap();
    assert_eq!(&buf[..4], &7u32.to_le_bytes());
    let mut c = std::io::Cursor::new(buf);
    assert_eq!(read_frame(&mut c).unwrap().unwrap(), b"{\"a\":1}");
    assert!(read_frame(&mut c).unwrap().is_none());
}
#[test] fn frame_too_large_rejected() {
    let mut c = std::io::Cursor::new(((MAX_FRAME as u32) + 1).to_le_bytes().to_vec());
    assert_eq!(read_frame(&mut c).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
}
#[test] fn frame_truncated_is_error() {
    let mut c = std::io::Cursor::new(vec![5, 0, 0, 0, b'a']);
    assert!(read_frame(&mut c).is_err());
}
#[test] fn parse_capture() {
    let r: Request = serde_json::from_str(r#"{"type":"capture","id":"7","url":"https://a.com/f.zip","filesize":10,"headers":{"Cookie":"x"}}"#).unwrap();
    match r { Request::Capture { id, item } => { assert_eq!(id, "7"); assert_eq!(item.filesize, 10); assert!(item.filename.is_none()); } _ => panic!() }
}
#[test] fn unknown_type_is_error_but_id_recoverable() {
    let raw = br#"{"type":"nope","id":"9"}"#;
    assert!(serde_json::from_slice::<Request>(raw).is_err());
    assert_eq!(request_id(raw).as_deref(), Some("9"));
}
#[test] fn response_shape() {
    let s = serde_json::to_string(&Response { id: "1".into(), ok: true, decision: Some(Decision::Accepted), ..Default::default() }).unwrap();
    assert_eq!(s, r#"{"id":"1","ok":true,"decision":"accepted"}"#);
}
#[test] fn header_allowlist() {
    let h: BTreeMap<_, _> = [("Cookie","a"),("referer","b"),("Authorization","c"),("X-Foo","d"),("User-Agent","e"),("Origin","f")]
        .into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let f = filter_headers(&h);
    assert_eq!(f.keys().cloned().collect::<Vec<_>>(), ["Cookie", "Origin", "Referer", "User-Agent"]);
}
#[test] fn pipe_name_has_sid() {
    let n = crate::winutil::pipe_name().unwrap();
    assert!(n.starts_with(r"\\.\pipe\openidm-S-1-"), "{n}");
}
```

- [ ] **Step 3:** Run `cargo test -p openidm-core protocol`. Expected: gagal kompilasi.
- [ ] **Step 4:** Implementasikan. `filter_headers` menormalkan nama ke bentuk kanonik di atas dan membuang nilai yang mengandung `\r` atau `\n`. `current_user_sid` memakai `windows-sys` (`OpenProcessToken`, `GetTokenInformation(TokenUser)`, `ConvertSidToStringSidW`).
- [ ] **Step 5:** Run ulang. Expected: 8 passed. Commit `feat(core): protokol Native Messaging dan nama pipe`.

---

### Task 4: `db.rs` dan `settings.rs`

**Files:**
- Create: `crates/openidm-core/src/db.rs`, `crates/openidm-core/src/settings.rs`

**Interfaces:**
- Produces:
  - ```rust
    #[derive(Serialize, Clone, Debug)]
    pub struct TaskRow { pub id: String, pub url: String, pub title: String, pub filename: String, pub filepath: String,
        pub filesize: u64, pub downloaded_bytes: u64, pub progress: f64, pub speed: f64, pub eta: u64,
        pub status: String, pub category: String, pub thumbnail: String, pub format_id: String,
        pub error_message: String, pub request_id: String, pub created_at: f64, pub completed_at: Option<f64>,
        #[serde(skip)] pub headers: BTreeMap<String, String>, pub can_resume: bool }
    pub struct Db;  // Clone + Send + Sync (Arc<Mutex<Connection>>)
    impl Db {
        pub fn open(path: &Path) -> rusqlite::Result<Db>;      // buat tabel + migrasi + WAL
        pub fn insert(&self, row: &TaskRow) -> rusqlite::Result<()>;
        pub fn get(&self, id: &str) -> rusqlite::Result<Option<TaskRow>>;
        pub fn list(&self, category: Option<&str>, status: Option<&str>) -> rusqlite::Result<Vec<TaskRow>>; // created_at DESC
        pub fn set_status(&self, id: &str, status: &str, error: &str) -> rusqlite::Result<()>;  // "completed" mengisi completed_at
        pub fn set_progress(&self, id: &str, downloaded: u64, filesize: u64, speed: f64, eta: u64) -> rusqlite::Result<()>;
        pub fn set_file(&self, id: &str, filepath: &str, filename: &str, filesize: u64) -> rusqlite::Result<()>;
        pub fn clear_cookies(&self, id: &str) -> rusqlite::Result<()>;   // hapus kunci "Cookie" dari JSON headers
        pub fn delete(&self, id: &str) -> rusqlite::Result<bool>;
        pub fn reset_running_to_paused(&self) -> rusqlite::Result<usize>; // downloading|pending -> paused
    }
    ```
    `can_resume` tidak disimpan; diisi `true` saat dibaca, lalu ditimpa manager untuk tugas aktif. Status: `pending`, `downloading`, `paused`, `completed`, `error`.
  - ```rust
    #[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] #[serde(default)]
    pub struct Settings { pub download_dir: PathBuf, pub max_concurrent: usize, pub segments: usize,
                          pub autostart: bool, pub confirm_dialog: bool }
    impl Default for Settings   // %USERPROFILE%\Downloads\OpenIDM, 3, 8, true, true
    impl Settings { pub fn load(data_dir: &Path) -> Settings; pub fn save(&self, data_dir: &Path) -> io::Result<()>;
                    pub fn validated(self) -> Result<Settings, String>; }
    ```
    `validated`: `max_concurrent` 1..=8, `segments` 1..=16, `download_dir` absolut; selain itu `Err` berbahasa Indonesia. `load` mengembalikan default bila berkas hilang atau rusak.

- [ ] **Step 1: Tulis test yang gagal:**

```rust
#[test] fn opens_legacy_db_and_adds_request_id() {
    let d = tempfile::tempdir().unwrap(); let p = d.path().join("downloads.db");
    { let c = rusqlite::Connection::open(&p).unwrap();
      c.execute_batch("CREATE TABLE downloads (id TEXT PRIMARY KEY, url TEXT NOT NULL, title TEXT NOT NULL, filename TEXT NOT NULL, filepath TEXT NOT NULL, filesize INTEGER DEFAULT 0, downloaded_bytes INTEGER DEFAULT 0, progress REAL DEFAULT 0.0, speed REAL DEFAULT 0.0, eta INTEGER DEFAULT 0, status TEXT DEFAULT 'pending', category TEXT DEFAULT 'General', thumbnail TEXT DEFAULT '', format_id TEXT DEFAULT '', error_message TEXT DEFAULT '', created_at REAL NOT NULL, completed_at REAL DEFAULT NULL, headers TEXT DEFAULT '');
        INSERT INTO downloads (id,url,title,filename,filepath,status,created_at,headers) VALUES ('old1','https://a/b','T','b.webm','C:/x/b.webm','completed',1.0,'{\"Cookie\":\"s\"}');").unwrap(); }
    let db = Db::open(&p).unwrap();
    let r = db.get("old1").unwrap().unwrap();
    assert_eq!((r.status.as_str(), r.request_id.as_str()), ("completed", ""));
    assert_eq!(r.headers["Cookie"], "s");
}
#[test] fn lifecycle() {
    let d = tempfile::tempdir().unwrap(); let db = Db::open(&d.path().join("x.db")).unwrap();
    db.insert(&row("a", "downloading", 1.0)).unwrap(); db.insert(&row("b", "pending", 2.0)).unwrap();
    assert_eq!(db.list(None, None).unwrap().iter().map(|r| r.id.clone()).collect::<Vec<_>>(), ["b", "a"]);
    db.set_progress("a", 50, 200, 10.0, 15).unwrap();
    let a = db.get("a").unwrap().unwrap(); assert_eq!((a.downloaded_bytes, a.filesize, a.progress), (50, 200, 25.0));
    assert_eq!(db.reset_running_to_paused().unwrap(), 2);
    db.set_status("a", "completed", "").unwrap(); assert!(db.get("a").unwrap().unwrap().completed_at.is_some());
    db.clear_cookies("a").unwrap(); assert!(!db.get("a").unwrap().unwrap().headers.contains_key("Cookie"));
    assert!(db.delete("a").unwrap()); assert!(!db.delete("a").unwrap());
}
#[test] fn settings_defaults_roundtrip_and_validation() {
    let d = tempfile::tempdir().unwrap();
    let s = Settings::load(d.path());
    assert_eq!((s.max_concurrent, s.segments, s.autostart, s.confirm_dialog), (3, 8, true, true));
    assert!(s.download_dir.ends_with("Downloads/OpenIDM") || s.download_dir.ends_with(r"Downloads\OpenIDM"));
    let mut t = s.clone(); t.max_concurrent = 2; t.save(d.path()).unwrap();
    assert_eq!(Settings::load(d.path()), t);
    std::fs::write(d.path().join("settings.json"), "{broken").unwrap();
    assert_eq!(Settings::load(d.path()), Settings::default());
    assert!(Settings { max_concurrent: 0, ..Settings::default() }.validated().is_err());
    assert!(Settings { download_dir: "relative".into(), ..Settings::default() }.validated().is_err());
}
```
(`row(id, status, created_at)` adalah helper test yang mengisi `TaskRow` dengan header `{"Cookie":"c","Referer":"r"}`.)

- [ ] **Step 2:** Run `cargo test -p openidm-core db settings`. Expected: gagal kompilasi.
- [ ] **Step 3:** Implementasikan. `rusqlite` dengan fitur `bundled`; `PRAGMA journal_mode=WAL; synchronous=NORMAL`; indeks sama dengan `core/database.py:47-50`; semua query berparameter.
- [ ] **Step 4:** Run ulang. Expected: 3 passed. Commit `feat(core): database dan pengaturan`.

---

### Task 5: Engine — probe, satu koneksi, multi-segmen, selesai

**Files:**
- Create: `crates/openidm-core/src/engine.rs`, `crates/openidm-core/tests/support/mod.rs`, `crates/openidm-core/tests/engine.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Job { pub id: String, pub url: url::Url, pub headers: BTreeMap<String, String>,
                   pub part_path: PathBuf,           // <final>.part ; state di <final>.part.json
                   pub final_path: PathBuf, pub expected_size: u64 }
  pub struct EngineConfig { pub segments: usize, pub min_segmented: u64, pub retries: u32,
                            pub retry_base: Duration, pub reserve: u64,
                            pub free_space: Arc<dyn Fn(&Path) -> io::Result<u64> + Send + Sync> }
  impl Default for EngineConfig  // 8, 2 MiB, 5, 500 ms, 100 MiB, fs4::available_space
  #[derive(Clone, Default)]
  pub struct Progress { /* atomik */ }
  impl Progress { pub fn downloaded(&self) -> u64; pub fn total(&self) -> u64; pub fn can_resume(&self) -> bool; }
  #[derive(Debug, PartialEq)]
  pub enum Outcome { Completed { path: PathBuf, size: u64 }, Paused }
  #[derive(Debug, thiserror::Error)]
  pub enum EngineError { Rejected(u16), Expired, DiskFull { drive: String, need: u64, free: u64 },
                         DirUnavailable(PathBuf), Network(String), Io(String) }
  impl EngineError { pub fn user_message(&self) -> String }   // Bahasa Indonesia
  pub async fn run(client: &reqwest::Client, job: &Job, cfg: &EngineConfig, progress: &Progress,
                   stop: tokio_util::sync::CancellationToken, first_byte: Option<tokio::sync::oneshot::Sender<()>>)
                   -> Result<Outcome, EngineError>
  pub fn build_client() -> reqwest::Client   // rustls, redirect ≤ 10, connect timeout 20 s, tanpa timeout total
  ```
  `stop` dibatalkan → `run` berhenti rapi, menyimpan state, mengembalikan `Outcome::Paused`. `first_byte` dikirim sekali saat byte isi pertama diterima.
- Test support (`tests/support/mod.rs`): `pub struct TestServer { pub url: String, pub hits: Arc<AtomicUsize> }` dan `pub async fn serve(body: Vec<u8>, opts: ServeOpts) -> TestServer` dengan `ServeOpts { ranges: bool, content_length: bool, status: Option<u16>, drop_after: Option<usize> /* putus setelah N byte per respons */, disposition: Option<String>, status_on_range_from: Option<(u64, u16)> /* status untuk Range yang mulai ≥ offset */ }`, dibangun di atas `axum`, port acak di `127.0.0.1`.

- [ ] **Step 1: Tulis test yang gagal** (`tests/engine.rs`; `body(n)` menghasilkan n byte pseudo-acak deterministik; `job(dir, url, size)` membuat `Job` dengan `final_path = dir/out.bin`):

```rust
#[tokio::test] async fn segmented_download_matches_source() {
    let data = body(5 * 1024 * 1024); let s = serve(data.clone(), ServeOpts { ranges: true, content_length: true, ..Default::default() }).await;
    let d = tempdir().unwrap(); let p = Progress::default();
    let out = run(&build_client(), &job(d.path(), &s.url, 0), &EngineConfig::default(), &p, CancellationToken::new(), None).await.unwrap();
    assert_eq!(out, Outcome::Completed { path: d.path().join("out.bin"), size: data.len() as u64 });
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(s.hits.load(Ordering::SeqCst) >= 9);                      // probe + 8 segmen
    assert!(!d.path().join("out.bin.part").exists() && !d.path().join("out.bin.part.json").exists());
    assert!(p.can_resume()); assert_eq!(p.downloaded(), data.len() as u64);
}
#[tokio::test] async fn small_file_uses_single_stream() {
    let data = body(100_000); let s = serve(data.clone(), ServeOpts { ranges: true, content_length: true, ..Default::default() }).await;
    let d = tempdir().unwrap();
    run(&build_client(), &job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default(), CancellationToken::new(), None).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(s.hits.load(Ordering::SeqCst) <= 2);
}
#[tokio::test] async fn no_range_unknown_length() {
    let data = body(3 * 1024 * 1024); let s = serve(data.clone(), ServeOpts { ranges: false, content_length: false, ..Default::default() }).await;
    let d = tempdir().unwrap(); let p = Progress::default();
    run(&build_client(), &job(d.path(), &s.url, 0), &EngineConfig::default(), &p, CancellationToken::new(), None).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(!p.can_resume());
}
#[tokio::test] async fn rejected_before_first_byte() {
    let s = serve(vec![], ServeOpts { status: Some(403), ..Default::default() }).await; let d = tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let e = run(&build_client(), &job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default(), CancellationToken::new(), Some(tx)).await.unwrap_err();
    assert!(matches!(e, EngineError::Rejected(403)));
    assert!(rx.try_recv().is_err()); assert!(!d.path().join("out.bin.part").exists());
}
#[tokio::test] async fn first_byte_signalled() {
    let s = serve(body(4096), ServeOpts { ranges: true, content_length: true, ..Default::default() }).await; let d = tempdir().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    run(&build_client(), &job(d.path(), &s.url, 0), &EngineConfig::default(), &Progress::default(), CancellationToken::new(), Some(tx)).await.unwrap();
    assert!(rx.await.is_ok());
}
#[tokio::test] async fn headers_forwarded() { /* server menyimpan header permintaan terakhir; assert Cookie == "k=v" dan Referer == "https://ref/" */ }
```

- [ ] **Step 2:** Run `cargo test -p openidm-core --test engine`. Expected: gagal kompilasi.
- [ ] **Step 3:** Implementasikan `run`:
  1. Buat folder induk `part_path`; gagal → `DirUnavailable`.
  2. Probe `GET` + `Range: bytes=0-0`. Status ≥ 400 → `Rejected(status)`. `206` + `Content-Range: bytes 0-0/N` → dukung Range, total N. `200` → tidak dukung Range, total dari `Content-Length` bila ada; badan respons ini langsung dipakai sebagai aliran satu koneksi (jangan minta ulang).
  3. Bila total diketahui: `free_space(folder) < total + reserve` → `DiskFull`.
  4. Dukung Range dan total ≥ `min_segmented` → `set_len(total)` pada `.part`, bagi rata `segments` rentang, satu task tokio per segmen, masing-masing membuka handle sendiri dan menulis di offset-nya. Selain itu → satu koneksi.
  5. Setelah semua selesai dan jumlah byte == total (bila diketahui): rename `.part` → `final_path`, hapus `.part.json`, kembalikan `Completed`.
- [ ] **Step 4:** Run ulang. Expected: 6 passed. Commit `feat(core): engine unduh multi-segmen`.

---

### Task 6: Engine — jeda/lanjut, coba ulang, fallback, disk

**Files:**
- Modify: `crates/openidm-core/src/engine.rs`, `crates/openidm-core/tests/engine.rs`

**Interfaces:**
- Produces: format `<final>.part.json`: `{"url": "...", "total": u64, "segments": [{"start": u64, "end": u64, "done": u64}]}` (`end` inklusif, `done` = byte yang sudah ditulis untuk segmen itu). Ditulis atomik (tulis ke `.tmp` lalu rename) setiap 1 detik dan saat berhenti. `EngineError::Expired` untuk 403/410 saat melanjutkan.

- [ ] **Step 1: Tulis test yang gagal:**

```rust
#[tokio::test] async fn pause_then_resume_completes_without_refetching_all() {
    let data = body(8 * 1024 * 1024);
    let s = serve(data.clone(), ServeOpts { ranges: true, content_length: true, throttle_bps: Some(4 * 1024 * 1024), ..Default::default() }).await;
    let d = tempdir().unwrap(); let j = job(d.path(), &s.url, 0); let p = Progress::default(); let stop = CancellationToken::new();
    let s2 = stop.clone(); let p2 = p.clone();
    tokio::spawn(async move { while p2.downloaded() < 1024 * 1024 { tokio::time::sleep(Duration::from_millis(20)).await } s2.cancel(); });
    assert_eq!(run(&build_client(), &j, &EngineConfig::default(), &p, stop, None).await.unwrap(), Outcome::Paused);
    let st: serde_json::Value = serde_json::from_slice(&std::fs::read(d.path().join("out.bin.part.json")).unwrap()).unwrap();
    let saved: u64 = st["segments"].as_array().unwrap().iter().map(|s| s["done"].as_u64().unwrap()).sum();
    assert!(saved >= 1024 * 1024 && saved < data.len() as u64);
    let served_before = s.bytes_served();
    let p = Progress::default();
    run(&build_client(), &j, &EngineConfig::default(), &p, CancellationToken::new(), None).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
    assert!(s.bytes_served() - served_before <= data.len() as u64 - saved + 64 * 1024);
}
#[tokio::test] async fn dropped_connections_are_retried() {
    let data = body(4 * 1024 * 1024);
    let s = serve(data.clone(), ServeOpts { ranges: true, content_length: true, drop_after: Some(300_000), ..Default::default() }).await;
    let d = tempdir().unwrap(); let cfg = EngineConfig { retries: 20, retry_base: Duration::from_millis(5), ..Default::default() };
    run(&build_client(), &job(d.path(), &s.url, 0), &cfg, &Progress::default(), CancellationToken::new(), None).await.unwrap();
    assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), data);
}
#[tokio::test] async fn retries_exhausted_keeps_part_and_state() {
    let s = serve(body(4 * 1024 * 1024), ServeOpts { ranges: true, content_length: true, drop_after: Some(1000), ..Default::default() }).await;
    let d = tempdir().unwrap(); let cfg = EngineConfig { retries: 2, retry_base: Duration::from_millis(5), ..Default::default() };
    let e = run(&build_client(), &job(d.path(), &s.url, 0), &cfg, &Progress::default(), CancellationToken::new(), None).await.unwrap_err();
    assert!(matches!(e, EngineError::Network(_)));
    assert!(d.path().join("out.bin.part").exists() && d.path().join("out.bin.part.json").exists());
}
#[tokio::test] async fn expired_link_on_resume() {           // Review Focus 1
    /* jeda seperti test pertama, lalu server baru di URL sama dengan status_on_range_from: Some((1, 403)) */
    let e = /* run kedua */; assert!(matches!(e, EngineError::Expired));
    assert_eq!(e.user_message(), "Tautan sudah kedaluwarsa. Unduh ulang dari website.");
    assert!(d.path().join("out.bin.part").exists());
}
#[tokio::test] async fn range_ignored_midway_falls_back_to_single() { /* server menjawab 200 badan penuh untuk Range non-probe; hasil akhir == data */ }
#[tokio::test] async fn missing_part_restarts_from_zero() { /* jeda, hapus .part (biarkan .json), run lagi; hasil == data */ }
#[tokio::test] async fn state_for_other_url_is_ignored() { /* .part.json dengan "url" berbeda → mulai dari nol; hasil == data */ }
#[tokio::test] async fn disk_full_reported() {
    let s = serve(body(1024), ServeOpts { ranges: true, content_length: true, ..Default::default() }).await; let d = tempdir().unwrap();
    let cfg = EngineConfig { free_space: Arc::new(|_| Ok(50 * 1024 * 1024)), ..Default::default() };
    let e = run(&build_client(), &job(d.path(), &s.url, 0), &cfg, &Progress::default(), CancellationToken::new(), None).await.unwrap_err();
    assert!(matches!(e, EngineError::DiskFull { free, .. } if free == 50 * 1024 * 1024));
    assert!(e.user_message().starts_with("Ruang disk tidak cukup di drive"));
}
#[tokio::test] async fn single_stream_pause_marks_not_resumable_and_restarts() { /* tanpa Range: jeda → Paused, can_resume()==false; run lagi → hasil == data */ }
```
Tambahkan ke `ServeOpts`/`TestServer`: `throttle_bps: Option<u64>` dan `fn bytes_served(&self) -> u64`.

- [ ] **Step 2:** Run `cargo test -p openidm-core --test engine`. Expected: 9 test baru gagal.
- [ ] **Step 3:** Implementasikan: muat state bila `.part` dan `.part.json` ada, `url` cocok, dan `total` sama dengan hasil probe; selain itu mulai dari nol. Coba ulang per segmen dengan jeda `retry_base * 2^n` (maks. 10 detik), penghitung di-reset setiap ada kemajuan. Status 403/410 pada permintaan saat state sudah ada → `Expired`. Respons 200 atas permintaan Range → batalkan segmen lain, mulai ulang satu koneksi. `user_message` untuk `DiskFull`: `"Ruang disk tidak cukup di drive {drive}. Kurang {x} MB."`; `DirUnavailable`: `"Folder unduhan tidak bisa diakses: {path}"`; `Rejected(s)`: `"Server menolak unduhan (HTTP {s})."`; `Network`: `"Koneksi terputus. Coba lanjutkan."`.
- [ ] **Step 4:** Run ulang. Expected: semua test engine lulus (15). Commit `feat(core): jeda/lanjut, coba ulang, dan cek disk`.

---

### Task 7: `manager.rs` — antrean dan siklus hidup tugas

**Files:**
- Create: `crates/openidm-core/src/manager.rs`, `crates/openidm-core/tests/manager.rs`

**Interfaces:**
- Consumes: `Db`, `TaskRow`, `Settings`, `engine::{run, Job, EngineConfig, Progress, Outcome, EngineError, build_client}`, `naming::*`, `protocol::filter_headers`.
- Produces:
  ```rust
  pub struct NewTask { pub url: String, pub filename: Option<String>, pub filesize: u64,
                       pub headers: BTreeMap<String, String>, pub request_id: Option<String>, pub start_paused: bool }
  pub struct Manager;   // dipakai sebagai Arc<Manager>
  impl Manager {
      pub fn open(data_dir: &Path) -> Result<Arc<Manager>, String>;   // buka DB, muat Settings, reset_running_to_paused
      pub fn with_engine_config(data_dir: &Path, cfg: EngineConfig) -> Result<Arc<Manager>, String>; // untuk test
      pub fn settings(&self) -> Settings;
      pub fn update_settings(&self, s: Settings) -> Result<Settings, String>;  // validated + save
      pub fn add(self: &Arc<Self>, t: NewTask) -> Result<TaskRow, String>;
      pub async fn wait_first_byte(&self, id: &str) -> Result<(), String>;    // Err(user_message) bila gagal sebelum byte pertama
      pub fn pause(&self, id: &str) -> bool;
      pub fn resume(self: &Arc<Self>, id: &str) -> bool;
      pub fn delete(&self, id: &str, delete_file: bool) -> bool;
      pub fn pause_all(&self); pub fn resume_all(self: &Arc<Self>);
      pub fn list(&self, category: Option<&str>, status: Option<&str>) -> Vec<TaskRow>;   // dengan angka live
      pub fn get(&self, id: &str) -> Option<TaskRow>;
      pub fn active_count(&self) -> usize;
  }
  ```
  `add`: validasi `http_url` (gagal → `"Tautan tidak valid."`), `filter_headers`, nama via `resolve_filename` + `category_for`, path `download_dir/<kategori>/<unique_path>` diverifikasi `is_inside`, `id` = 8 heksadesimal acak, `title` = nama file, `format_id = "direct"`. Status awal `paused` bila `start_paused`, selain itu `pending` lalu dijadwalkan. Penjadwal menjalankan paling banyak `max_concurrent` tugas; saat satu selesai/gagal/dijeda, tugas `pending` tertua mulai. Selesai → `set_file`, `set_status("completed")`, `clear_cookies`. Gagal → `set_status("error", user_message)`. `delete` menghentikan tugas, menghapus baris, dan menghapus `.part`/`.part.json`; file akhir hanya bila `delete_file`. Kecepatan = rata-rata bergerak 5 detik; `eta` = sisa/kecepatan. Progres ditulis ke DB tiap 1 detik.

- [ ] **Step 1: Tulis test yang gagal** (`setup()` → `(tempdir, Arc<Manager>)` dengan `download_dir` di dalam tempdir dan `max_concurrent = 3`; `wait_status(&m, id, status)` polling sampai 10 detik):

```rust
#[tokio::test] async fn add_completes_and_scrubs_cookies() {
    let data = body(300_000); let s = serve(data.clone(), ok()).await; let (d, m) = setup();
    let t = m.add(NewTask { url: format!("{}/report.pdf", s.url), headers: hdr(&[("Cookie", "k=v"), ("X-Evil", "1")]), ..nt() }).unwrap();
    assert_eq!((t.category.as_str(), t.format_id.as_str()), ("Documents", "direct"));
    m.wait_first_byte(&t.id).await.unwrap(); wait_status(&m, &t.id, "completed").await;
    let r = m.get(&t.id).unwrap();
    assert_eq!(std::fs::read(&r.filepath).unwrap(), data);
    assert!(std::path::Path::new(&r.filepath).starts_with(d.path().join("dl").join("Documents")));
    assert!(r.headers.is_empty());
}
#[tokio::test] async fn rejects_bad_url() { let (_d, m) = setup(); assert_eq!(m.add(NewTask { url: "file:///c:/x".into(), ..nt() }).unwrap_err(), "Tautan tidak valid."); }
#[tokio::test] async fn traversal_filename_stays_inside() {
    let s = serve(body(10), ok()).await; let (d, m) = setup();
    let t = m.add(NewTask { url: s.url.clone(), filename: Some("..\\..\\evil.exe".into()), ..nt() }).unwrap();
    assert!(std::path::Path::new(&t.filepath).starts_with(d.path().join("dl")));
    assert_eq!(t.filename, "evil.exe");
}
#[tokio::test] async fn concurrency_limit_and_promotion() {
    let s = serve(body(6 * 1024 * 1024), ServeOpts { throttle_bps: Some(1024 * 1024), ..ok() }).await; let (_d, m) = setup();
    let ids: Vec<_> = (0..5).map(|i| m.add(NewTask { url: format!("{}/f{i}.bin", s.url), ..nt() }).unwrap().id).collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(m.active_count(), 3);
    assert_eq!(m.list(None, Some("pending")).len(), 2);
    assert!(m.pause(&ids[0])); wait_status(&m, &ids[0], "paused").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(m.active_count(), 3);                 // satu pending naik
}
#[tokio::test] async fn failure_before_first_byte() {
    let s = serve(vec![], ServeOpts { status: Some(404), ..Default::default() }).await; let (_d, m) = setup();
    let t = m.add(NewTask { url: s.url.clone(), ..nt() }).unwrap();
    assert_eq!(m.wait_first_byte(&t.id).await.unwrap_err(), "Server menolak unduhan (HTTP 404).");
    assert_eq!(m.get(&t.id).unwrap().status, "error");
}
#[tokio::test] async fn start_paused_does_not_download() {
    let s = serve(body(1000), ok()).await; let (_d, m) = setup();
    let t = m.add(NewTask { url: s.url.clone(), start_paused: true, ..nt() }).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(m.get(&t.id).unwrap().status, "paused"); assert_eq!(s.hits.load(Ordering::SeqCst), 0);
    assert!(m.resume(&t.id)); wait_status(&m, &t.id, "completed").await;
}
#[tokio::test] async fn restart_marks_running_paused_then_resumes() { /* add throttled, drop Manager di tengah, Manager::open lagi → status "paused"; resume → completed, isi == data */ }
#[tokio::test] async fn delete_removes_partials_keeps_final_unless_asked() { /* dua kasus */ }
#[tokio::test] async fn duplicate_name_gets_suffix() { /* dua add URL sama → "a.zip" dan "a (1).zip" */ }
#[tokio::test] async fn download_dir_unavailable() {           // Review Focus 5
    let s = serve(body(10), ok()).await; let (d, m) = setup();
    let blocker = d.path().join("blocked"); std::fs::write(&blocker, b"x").unwrap();   // file, bukan folder
    m.update_settings(Settings { download_dir: blocker.clone(), ..m.settings() }).unwrap();
    let t = m.add(NewTask { url: s.url.clone(), ..nt() }).unwrap(); wait_status(&m, &t.id, "error").await;
    assert!(m.get(&t.id).unwrap().error_message.starts_with("Folder unduhan tidak bisa diakses:"));
}
```

- [ ] **Step 2:** Run `cargo test -p openidm-core --test manager`. Expected: gagal kompilasi.
- [ ] **Step 3:** Implementasikan sesuai Interfaces.
- [ ] **Step 4:** Run `cargo test -p openidm-core`. Expected: semua lulus. Commit `feat(core): manajer antrean unduhan`.

---

### Task 8: `openidm-host` — relay Native Messaging

**Files:**
- Create: `crates/openidm-host/Cargo.toml`, `crates/openidm-host/src/main.rs`, `crates/openidm-host/src/relay.rs`, `crates/openidm-host/tests/relay.rs`
- Modify: `Cargo.toml` (tambah anggota)

**Interfaces:**
- Consumes: `protocol::{read_frame, write_frame, request_id}`, `winutil::pipe_name`.
- Produces: `openidm-host.exe`. Wire pada pipe sama persis dengan Native Messaging (frame 4 byte LE + JSON); host tidak mengurai isi selain mengambil `id`.
  ```rust
  pub trait Connector { fn connect(&mut self) -> io::Result<Box<dyn ReadWrite>>; fn launch_app(&mut self) -> io::Result<()>; }
  pub fn relay<I: Read, O: Write + Send>(stdin: I, stdout: O, c: &mut dyn Connector, wait: Duration) -> io::Result<()>
  ```
  Perilaku `relay`: sambung saat frame pertama datang. Gagal sambung → `launch_app()` sekali, coba ulang tiap 200 ms sampai `wait` (5 detik di `main`). Tetap gagal → untuk frame itu dan setiap frame berikutnya balas `{"id":<id>,"ok":false,"error":"app_unavailable"}`. Host mencatat `id` yang sudah diteruskan dan belum dibalas; bila pipe putus, kirim `app_unavailable` untuk tiap `id` itu, lalu coba sambung ulang pada frame berikutnya. EOF stdin → keluar 0.
  `main`: connector nyata membuka pipe sebagai file; `launch_app` menjalankan `OpenIDM.exe --background` dari folder exe host dengan `DETACHED_PROCESS | CREATE_NO_WINDOW`. stdout hanya untuk frame; jangan pernah `println!`.

- [ ] **Step 1: Tulis test yang gagal** (connector tiruan di atas pasangan pipa in-memory; `frames(bytes)` mengurai keluaran stdout):

```rust
#[test] fn forwards_both_directions() { /* app tiruan menggemakan {"id":x,"ok":true}; kirim 2 frame; stdout berisi 2 balasan dengan id cocok */ }
#[test] fn launches_app_when_pipe_missing() { /* connect gagal 2x lalu berhasil; assert launch_app dipanggil tepat 1 kali; balasan diterima */ }
#[test] fn app_unavailable_after_wait() {
    let out = run_relay(frames_in(&[r#"{"type":"ping","id":"a"}"#, r#"{"type":"ping","id":"b"}"#]), NeverConnects::default(), Duration::from_millis(300));
    assert_eq!(out, [json!({"id":"a","ok":false,"error":"app_unavailable"}), json!({"id":"b","ok":false,"error":"app_unavailable"})]);
}
#[test] fn pipe_drop_fails_pending_ids() {                    // Review Focus 3
    /* app tiruan menerima frame id "c" lalu menutup pipa tanpa membalas */
    assert_eq!(out, [json!({"id":"c","ok":false,"error":"app_unavailable"})]);
}
#[test] fn oversized_or_garbage_stdin_exits_cleanly() { /* panjang > MAX_FRAME → relay mengembalikan Err, tidak panic, stdout kosong */ }
```

- [ ] **Step 2:** Run `cargo test -p openidm-host`. Expected: gagal kompilasi.
- [ ] **Step 3:** Implementasikan dengan `std` saja (dua thread: stdin→pipe dan pipe→stdout, stdout dilindungi `Mutex`).
- [ ] **Step 4:** Run ulang. Expected: 5 passed. Commit `feat(host): relay Native Messaging`.

---

### Task 9: App Tauri — jendela, perintah, UI di dalam app

**Files:**
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/tray.rs`, `ui/api.js`
- Modify: `Cargo.toml`, `ui/index.html`, `ui/app.js` (titik `fetch` di baris 139-143, 380, 410, 424, 438, 450, 532, 631, 750, 776 dan interval di 828-829), `ui/styles.css`

**Interfaces:**
- Consumes: `Manager` (Task 7).
- Produces:
  - Perintah Tauri (semua mengembalikan `Result<_, String>` dengan pesan Indonesia):
    `get_status() -> { version, active_downloads, total_downloads, disk: { free, total, percent } }` (disk dari drive `download_dir`),
    `list_downloads(category: Option<String>, status: Option<String>) -> Vec<TaskRow>`,
    `add_download(url: String, filename: Option<String>) -> TaskRow`,
    `control_download(id: String, action: String, delete_file: Option<bool>) -> bool` (`pause` | `resume` | `delete`; `cancel` diperlakukan sebagai `pause`),
    `open_file(id: String) -> bool`, `open_folder(id: String) -> bool` (hanya path dari baris DB; `open_folder` memakai `explorer /select,`),
    `get_info(url: String)` → selalu `Err("Analisis video dan folder Google Drive belum tersedia di aplikasi ini. Gunakan OpenIDM Legacy untuk sementara.")` untuk `corePage`/stream; untuk URL file langsung mengembalikan `{ filename, category, direct: true }`,
    `get_settings() -> Settings`, `save_settings(settings: Settings) -> Settings`, `pick_folder() -> Option<String>`, `quit_app()`.
  - `window.OpenIDMApi` di `ui/api.js` dengan metode bernama sama dalam camelCase (`getStatus`, `listDownloads`, …), membungkus `window.__TAURI__.core.invoke`.
  - Argumen CLI `--background`: app mulai tanpa menampilkan jendela utama.
  - `tray::show_main(app: &AppHandle)` — tampilkan dan fokuskan jendela utama (dipakai Task 10).

- [ ] **Step 1:** Lewat `context7`, baca dokumentasi Tauri 2 untuk: `tauri.conf.json` (`frontendDist`, `app.withGlobalTauri`, `app.security.csp`), capabilities/permissions, tray (`TrayIconBuilder`), event `CloseRequested`, dan plugin `single-instance`, `dialog`, `autostart`. Catat versi crate yang dipakai di `src-tauri/Cargo.toml`.
- [ ] **Step 2:** Scaffold `src-tauri` tanpa Node: `frontendDist = "../ui"`, `withGlobalTauri = true`, tanpa `devUrl`, `identifier = "id.openidm.desktop"`, jendela `main` 1180×760 (min 920×600) judul "OpenIDM", ikon dari `icon.ico`, CSP `default-src 'self'; img-src 'self' data: https:; style-src 'self' 'unsafe-inline'`. Build tanpa tauri-cli: `cargo build -p openidm --release --features tauri/custom-protocol`. Nama biner `OpenIDM`. `data_dir` = folder exe.
- [ ] **Step 3:** Tulis test unit untuk logika murni di `commands.rs` dan jalankan (gagal dulu):

```rust
#[test] fn control_action_validation() {
    assert!(parse_action("pause").is_ok() && parse_action("resume").is_ok() && parse_action("delete").is_ok());
    assert_eq!(parse_action("cancel").unwrap(), Action::Pause);
    assert_eq!(parse_action("rm -rf").unwrap_err(), "Perintah tidak dikenal.");
}
#[test] fn info_routes_core_pages_to_legacy_message() {
    assert!(classify_info("https://www.youtube.com/watch?v=x").unwrap_err().contains("belum tersedia"));
    assert!(classify_info("https://drive.google.com/drive/folders/abc").unwrap_err().contains("belum tersedia"));
    assert!(classify_info("https://a.com/x.m3u8").unwrap_err().contains("belum tersedia"));
    assert_eq!(classify_info("https://a.com/setup.exe").unwrap().category, "Programs");
    assert_eq!(classify_info("notaurl").unwrap_err(), "Tautan tidak valid.");
}
```
  Daftar host `corePage` sama dengan `browser-extension/shared.js:14`.
- [ ] **Step 4:** Implementasikan perintah, single instance (instance kedua → `show_main`), tutup jendela = sembunyi (`prevent_close` + `hide`), tray dengan menu "Buka OpenIDM", "Jeda semua", "Lanjutkan semua", "Keluar"; klik kiri → `show_main`. Run `cargo test -p openidm`. Expected: 2 passed.
- [ ] **Step 5:** `ui/api.js` + ubah `app.js`: setiap `fetch("/api/…")` diganti panggilan `OpenIDMApi`; bentuk data yang dikembalikan disamakan dengan yang dipakai `app.js` sekarang (`{ downloads }`, `{ success }`) di dalam adapter agar perubahan `app.js` minimal. Tombol matikan server → "Keluar" (`quitApp`). Indikator "Core Engine Terhubung / FFmpeg Siap" → "Siap" dengan jumlah unduhan aktif. Hapus penanganan `?addUrl=` dari query (diganti event di Task 10).
- [ ] **Step 6:** Halaman Pengaturan (tombol roda gigi yang sudah ada): folder simpan + tombol "Pilih folder", jumlah unduhan bersamaan (1–8), autostart, dialog konfirmasi. Tombol Simpan punya state loading dan disabled; error validasi ditampilkan di bawah field. Ikuti kelas dan token warna `styles.css` yang ada.
- [ ] **Step 7:** Build rilis dan jalankan `D:\rust\target\openidm\release\OpenIDM.exe` dengan salinan `downloads.db` di sampingnya. Verifikasi manual: jendela app sendiri (bukan browser) tampil dengan 21 riwayat; "Tambah URL" dengan `https://proof.ovh.net/files/10Mb.dat` selesai dan MD5 file sama dengan unduhan `curl`; jeda/lanjut bekerja; tutup jendela → tetap di tray; "Keluar" menghentikan proses; tidak ada listener di port 6899 milik proses ini (`netstat -ano | findstr <PID>` kosong).
- [ ] **Step 8:** Commit `feat(app): aplikasi desktop Tauri dengan UI lama`.

---

### Task 10: Server pipe, dialog konfirmasi, pendaftaran browser, autostart

**Files:**
- Create: `src-tauri/src/pipe.rs`, `src-tauri/src/capture.rs`, `src-tauri/src/register.rs`, `ui/confirm.html`, `ui/confirm.js`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/commands.rs`, `ui/app.js`, `ui/index.html`, `ui/styles.css`

**Interfaces:**
- Consumes: `protocol::*`, `winutil::pipe_name`, `Manager`, `tray::show_main`.
- Produces:
  - ```rust
    // capture.rs — tanpa ketergantungan Tauri agar bisa diuji
    #[derive(Clone, Copy, PartialEq, Debug)] pub enum Choice { Start, Later, Cancel }
    #[async_trait] pub trait Prompter: Send + Sync { async fn ask(&self, key: &str, item: &CaptureItem, suggested: &Suggested) -> (Choice, Option<String> /* nama file hasil edit */); }
    pub struct Suggested { pub filename: String, pub category: &'static str, pub dir: PathBuf }
    pub async fn handle(req: Request, m: &Arc<Manager>, p: &dyn Prompter, ui: &dyn UiHooks) -> Response
    pub trait UiHooks: Send + Sync { fn show_main(&self, add_url: Option<String>); }
    ```
    Aturan `handle`: `Ping` → `ok`, `version`, `capabilities = ["capture", "add", "show"]`. `Add` → `m.add` → `task_id` atau `ok:false` + `error`. `Show` → `ui.show_main(add_url yang lolos http_url)`. `Capture` → bila `settings.confirm_dialog` tanyakan `p.ask`, selain itu `Start`. `Start` → `m.add` lalu `wait_first_byte`: sukses → `Accepted`; gagal → `m.delete(id, false)` dan `Failed` + `error`. `Later` → `m.add(start_paused: true)` → `Later`. `Cancel` → `Declined`. URL tidak valid → `ok:false, error:"Tautan tidak valid."`.
  - `pipe::serve(app: AppHandle, m: Arc<Manager>)` — named pipe tokio, `first_pipe_instance(true)`, `reject_remote_clients(true)`, security descriptor dari SDDL `D:P(A;;GA;;;<SID>)`; setiap koneksi dilayani task sendiri; setiap frame diproses di task terpisah sehingga `capture` yang menunggu dialog tidak memblokir pesan lain; penulisan balasan diserialkan. Frame tak terurai → `{id, ok:false, error:"Permintaan tidak dikenal."}` bila `id` bisa diambil, selain itu diabaikan.
  - Prompter Tauri: membuka jendela `confirm-<key>` (460×300, always-on-top, tidak resizable, di tengah, fokus) yang memuat `confirm.html?key=<key>`; perintah `confirm_get(key) -> { url_host, filename, filesize, category, dir }` dan `confirm_decide(key, choice, filename)`. Jendela ditutup tanpa memilih → `Cancel`.
  - `register::register(exe_dir: &Path) -> Result<(), String>` menulis `openidm-host.json` (`name`, `description`, `path` absolut ke `openidm-host.exe`, `type: "stdio"`, `allowed_origins: ["chrome-extension://<EXTENSION_ID>/"]`) dan nilai default kunci `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.openidm.host` serta `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.openidm.host`. `register::status() -> { chrome: bool, edge: bool, host_exists: bool }`. Perintah Tauri: `browser_status()`, `register_browser()`.
  - Autostart: plugin autostart dengan argumen `--background`, disinkronkan dengan `settings.autostart` saat start dan saat `save_settings`.
  - Event `openidm://add-url` (payload string) ke jendela utama → `app.js` membuka dialog "Tambah URL" terisi.

- [ ] **Step 1: Tulis test yang gagal** (`capture.rs`, dengan `FakePrompter` yang jawabannya diatur per `url` dan bisa ditahan dengan `Notify`):

```rust
#[tokio::test] async fn start_returns_accepted_after_first_byte() { /* server ok; Choice::Start → decision Accepted; tugas ada di manager */ }
#[tokio::test] async fn start_but_server_rejects_returns_failed_and_removes_task() {
    /* server 403 */ assert_eq!(json(&r)["decision"], "failed"); assert_eq!(json(&r)["error"], "Server menolak unduhan (HTTP 403)."); assert!(m.list(None, None).is_empty());
}
#[tokio::test] async fn later_creates_paused_task() { /* decision "later"; status "paused"; server hits == 0 */ }
#[tokio::test] async fn cancel_declines_and_creates_nothing() { /* decision "declined"; list kosong */ }
#[tokio::test] async fn edited_filename_is_used_and_sanitized() { /* prompter mengembalikan Some("..\\baru.zip") → filename "baru.zip" */ }
#[tokio::test] async fn confirm_disabled_skips_prompt() { /* settings.confirm_dialog=false → prompter tidak dipanggil; Accepted */ }
#[tokio::test] async fn two_captures_answered_independently() {      // Review Focus 2
    /* kirim capture A lalu B bersamaan; jawab B=Cancel dulu, lalu A=Start */
    assert_eq!(resp_b.id, "B"); assert_eq!(json(&resp_b)["decision"], "declined");
    assert_eq!(resp_a.id, "A"); assert_eq!(json(&resp_a)["decision"], "accepted");
}
#[tokio::test] async fn invalid_url_rejected_without_prompt() { /* url "javascript:1" → ok:false; prompter tidak dipanggil */ }
#[tokio::test] async fn ping_and_show() { /* ping: ok + capabilities; show dengan add_url "ftp://x" → show_main(None) */ }
```
  Dan di `register.rs`:
```rust
#[test] fn manifest_content() {
    let d = tempdir().unwrap(); let v = host_manifest(d.path());
    assert_eq!(v["name"], "com.openidm.host"); assert_eq!(v["type"], "stdio");
    assert_eq!(v["allowed_origins"], json!([format!("chrome-extension://{EXTENSION_ID}/")]));
    assert!(std::path::Path::new(v["path"].as_str().unwrap()).is_absolute());
}
```

- [ ] **Step 2:** Run `cargo test -p openidm`. Expected: gagal kompilasi.
- [ ] **Step 3:** Implementasikan `capture.rs`, `register.rs`, lalu `pipe.rs` dan prompter Tauri. `confirm.html`: nama file (input teks, bisa diubah), host asal, ukuran (`Tidak diketahui` bila 0; format `654,4 MB`), kategori, folder; tombol "Mulai unduh" (utama, fokus awal, Enter), "Nanti", "Batal" (Esc). Tanpa emoji; ikon SVG.
- [ ] **Step 4:** Run `cargo test -p openidm`. Expected: semua lulus.
- [ ] **Step 5:** Di Pengaturan tambahkan bagian "Integrasi browser": status Chrome/Edge (Terdaftar/Belum) dan tombol "Daftarkan ulang".
- [ ] **Step 6:** Verifikasi manual tanpa browser: build rilis kedua exe ke satu folder; jalankan `OpenIDM.exe`; cek `reg query HKCU\Software\Google\Chrome\NativeMessagingHosts\com.openidm.host` menunjuk `openidm-host.json`. Lalu dengan skrip Node kecil di scratchpad, spawn `openidm-host.exe`, kirim frame `ping` → balasan `ok:true`; kirim `capture` untuk `https://proof.ovh.net/files/10Mb.dat` → dialog muncul; klik "Mulai unduh" → balasan `accepted` dan file selesai. Ulangi dengan `OpenIDM.exe` tidak berjalan → app menyala sendiri dan dialog muncul.
- [ ] **Step 7:** Commit `feat(app): auto-capture lewat named pipe, dialog konfirmasi, pendaftaran browser`.

---

### Task 11: Extension — Native Messaging

**Files:**
- Create: `browser-extension/native.js`
- Modify: `browser-extension/manifest.json`, `browser-extension/background.js`, `browser-extension/capture.js`, `browser-extension/popup.js`, `browser-extension/popup.html`, `tests/extension.test.cjs`

**Interfaces:**
- Consumes: protokol Task 3; `key` dan ID dari Task 3 Step 1.
- Produces:
  - `native.js` (di-`importScripts` sebelum `capture.js`): `OpenIDMNative.request(message, { timeout }) -> Promise<response>`; satu port `chrome.runtime.connectNative("com.openidm.host")` dibuat malas dan dibuat ulang setelah `onDisconnect`; `id` = penghitung + waktu; `timeout` opsional (tanpa batas bila tidak diisi); saat port putus semua permintaan tertunda ditolak dengan `Error` ber-`code = "app_unavailable"`; respons `ok:false` menolak dengan `error` dari app dan `code = response.error`.
  - `manifest.json`: `"version": "2.0.0"`, tambah `"nativeMessaging"` ke `permissions`, tambah `"key"`.

- [ ] **Step 1:** Baca `tests/extension.test.cjs` untuk mengikuti cara test yang ada memuat skrip worker dan meniru `chrome.*`. Tambahkan tiruan `chrome.runtime.connectNative` yang bisa diprogram per test (balasan, penundaan, putus).
- [ ] **Step 2: Tulis test yang gagal** di `tests/extension.test.cjs`:

```js
test("capture accepted: browser download dibatalkan, tidak ada tab dibuka", async () => {
  native.reply = m => ({ id: m.id, ok: true, decision: "accepted" });
  await fireDownloadCreated({ id: 1, url: "https://a.com/f.zip", filename: "f.zip", totalBytes: 10, state: "in_progress" });
  assert.deepEqual(calls.downloads, [["pause", 1], ["cancel", 1]]);
  assert.equal(calls.tabsCreate.length, 0);
  assert.equal(native.sent[0].type, "capture");
  assert.equal(native.sent[0].url, "https://a.com/f.zip");
});
test("capture later: diperlakukan seperti accepted", ...        // pause, cancel
test("capture declined: unduhan browser dilanjutkan tanpa notifikasi", ...   // pause, resume; notifications == 0
test("capture failed: dilanjutkan + notifikasi", ...            // pause, resume; 1 notifikasi berisi error dari app
test("app_unavailable: dilanjutkan + notifikasi 'Buka OpenIDM sekali'", ...
test("port putus sebelum balasan: unduhan browser dilanjutkan", ...          // Review Focus 3
test("capture tanpa batas waktu: balasan setelah 120 detik (fake timers) tetap diproses", ...
test("mode browser: tidak dijeda dan tidak ada pesan native", ...
test("POST atau Authorization: dilewati", ...
test("unduhan yang dimulai extension sendiri: dilewati", ...
test("header: hanya Cookie/Referer/User-Agent/Origin milik URL itu yang dikirim", ...
test("pemulihan: record 'submitting' + unduhan masih terjeda setelah worker restart → dilanjutkan", ...
test("menu klik kanan file langsung → pesan 'add'; halaman YouTube → POST HTTP ke 127.0.0.1:6899", ...
test("OPEN_DASHBOARD → pesan 'show', bukan tabs.create", ...
```

- [ ] **Step 3:** Run `node --test tests/extension.test.cjs`. Expected: test baru gagal; catat test lama mana yang ikut gagal karena alur berubah.
- [ ] **Step 4:** Implementasikan:
  - `capture.js`: ganti blok `status()` + `api("/download")` + `settleCapture` dengan satu `OpenIDMNative.request({ type: "capture", … })`. Hasil → tindakan: `accepted`/`later` → simpan fase `accepted`, `downloads.cancel`; `declined` → `resume`; `failed` → `resume` + notifikasi berisi `error`; `app_unavailable` → `resume` + notifikasi "OpenIDM tidak bisa dibuka. Buka OpenIDM sekali untuk mendaftarkan integrasi; unduhan dilanjutkan di browser." Hapus `showCapturedDashboard`, `capturedTask`, `dashboardUrl`. `recoverCaptures`: fase `accepted` → cancel; fase lain dengan unduhan masih terjeda → resume (worker yang restart kehilangan port, jadi tidak ada balasan yang akan datang).
  - `background.js`: `download()` merutekan `direct` → `OpenIDMNative.request({ type: "add", … })`; non-direct tetap `api("/download")` ke core lama dengan pesan error "Video dan folder Google Drive masih memakai OpenIDM Legacy. Jalankan legacy\OpenIDM.exe." bila offline. `CHECK_SERVER` mengembalikan `{ online, legacy }` (`ping` native dan `/status` HTTP). `OPEN_DASHBOARD` → `show`.
  - `popup.js`/`popup.html`: indikator status memakai `online` dari app Rust; teks menyebut "OpenIDM" bukan "OpenIDM.exe/Desktop core".
- [ ] **Step 5:** Run `node --test tests/extension.test.cjs`. Expected: semua lulus; test lama yang menguji alur HTTP capture diganti, bukan dihapus tanpa pengganti.
- [ ] **Step 6:** Commit `feat(extension): auto-capture lewat Native Messaging`.

---

### Task 12: Build, rilis ke `open-idm`, dokumentasi, uji nyata

**Files:**
- Create: `build-rust.bat`
- Modify: `README.md`, `browser-extension/INSTALL.md`, `C:\xampp\htdocs\open-idm\CARA-PAKAI.txt`, `C:\xampp\htdocs\open-idm\Jalankan-OpenIDM.bat`, `C:\xampp\htdocs\open-idm\Hentikan-OpenIDM.bat`

**Interfaces:**
- Consumes: semua task sebelumnya.

- [ ] **Step 1:** `build-rust.bat`: set `RUSTUP_HOME`/`CARGO_HOME`/PATH, jalankan `cargo test --workspace`, `cargo build --release -p openidm --features tauri/custom-protocol`, `cargo build --release -p openidm-host`, lalu salin `OpenIDM.exe`, `openidm-host.exe`, dan `browser-extension\` ke folder tujuan (argumen pertama, default `C:\xampp\htdocs\open-idm`). Berhenti dengan kode ≠ 0 bila ada langkah gagal.
- [ ] **Step 2:** Migrasi folder rilis (minta konfirmasi pengguna sebelum menghentikan proses yang sedang berjalan): hentikan `OpenIDM.exe` lama; buat `open-idm\legacy\`; **pindahkan** ke dalamnya `OpenIDM.exe`, `_internal\`, `ui\`, `bin\`; **salin** `downloads.db` ke `legacy\downloads.db` (setelah proses berhenti agar WAL sudah tergabung). `OpenIDM.exe.bak`, `data\`, `downloads\` tidak disentuh. Tidak ada yang dihapus.
- [ ] **Step 3:** Jalankan `build-rust.bat`. Expected: semua test lulus, kedua exe ada di `open-idm\`, ukuran `OpenIDM.exe` < 25 MB.
- [ ] **Step 4:** Perbarui `Jalankan-OpenIDM.bat` (menjalankan `OpenIDM.exe`), `Hentikan-OpenIDM.bat` (`taskkill /IM OpenIDM.exe`), `CARA-PAKAI.txt`, `README.md`, `INSTALL.md`: cara pasang ulang extension (ID berubah karena `key`, jadi hapus yang lama lalu Load unpacked), pembagian fitur app Rust vs `legacy\OpenIDM.exe`, lokasi pengaturan, dan cara build.
- [ ] **Step 5: Uji nyata di Chrome** (pengguna memuat ulang extension dari `open-idm\browser-extension`). Tiap butir harus diamati, bukan diasumsikan:
  1. Klik tautan file (mis. `https://proof.ovh.net/files/100Mb.dat`) → dialog OpenIDM muncul, tidak ada tab `127.0.0.1` yang terbuka → "Mulai unduh" → selesai; unduhan Chrome berstatus dibatalkan; ukuran file benar.
  2. "Batal" di dialog → unduhan berlanjut di Chrome.
  3. "Nanti" → tugas dijeda di OpenIDM; "Lanjutkan" → selesai.
  4. Keluar dari OpenIDM lewat tray, klik unduhan lagi → app menyala sendiri dan dialog muncul.
  5. Jeda di tengah, keluar app, buka lagi, lanjutkan → selesai tanpa mulai dari nol.
  6. Unduh arsip dari Google Takeout atau file Google Drive lewat tombol unduh di browser → berjalan sebagai file langsung (tidak ada pesan "Buka folder Google Drive…").
  7. Restart Windows (atau sign out/in) → OpenIDM ada di tray tanpa jendela.
  8. `legacy\OpenIDM.exe` dijalankan → tombol "Download this video" di YouTube masih bekerja.
- [ ] **Step 6:** Catat hasil tiap butir Step 5 (lulus/gagal + yang terlihat) dalam pesan serah terima. Butir yang gagal diperbaiki atau dilaporkan apa adanya.
- [ ] **Step 7:** Commit `chore: skrip build, migrasi folder rilis, dokumentasi Tahap 1`.
