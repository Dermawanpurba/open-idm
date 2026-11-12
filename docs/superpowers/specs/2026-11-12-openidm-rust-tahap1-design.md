# OpenIDM Rust — Tahap 1: App Desktop + Auto-Capture File Langsung

Tanggal: 2026-11-12
Status: menunggu tinjauan pemilik proyek

## 1. Tujuan

1. Klik download di website otomatis ditangani OpenIDM, seperti IDM.
2. OpenIDM menjadi aplikasi desktop sendiri berbasis Rust. Dashboard tidak lagi dibuka sebagai tab `http://127.0.0.1:6899/`.

Tahap 1 berhasil bila: pengguna mengklik tautan unduhan file di Chrome atau Edge, dialog OpenIDM muncul, dan setelah "Mulai" file selesai terunduh oleh app Rust tanpa ada tab dashboard yang terbuka dan tanpa proses Python.

## 2. Ruang lingkup

Termasuk Tahap 1:

- App desktop Tauri v2 (`OpenIDM.exe`) dengan jendela utama, tray, autostart.
- Engine unduh file langsung multi-segmen di Rust, dengan jeda/lanjut.
- Pengalihan otomatis unduhan browser lewat Native Messaging, dengan dialog konfirmasi.
- Unduhan manual: tombol "Tambah URL", menu klik kanan, dan popup extension untuk file langsung.
- Halaman Pengaturan.
- Riwayat lama di `downloads.db` tetap terbaca.

Tidak termasuk (tahap berikutnya):

- Tahap 2: video platform, yt-dlp, HLS/DASH, pilihan kualitas, tombol "Download this video".
- Tahap 3: folder Google Drive dan batch download.

Selama Tahap 1, fitur Tahap 2 dan 3 tetap dilayani core Python lama lewat HTTP `127.0.0.1:6899` bila core itu dijalankan. Kode Python tidak diubah dan tidak dihapus.

## 3. Keputusan

| Hal | Keputusan |
|---|---|
| Kerangka app | Tauri v2, UI HTML/JS yang ada dipakai ulang |
| Jalur extension ↔ app | Native Messaging; tanpa server HTTP di app baru |
| Perilaku klik download | Dialog konfirmasi: Mulai / Nanti / Batal |
| Siklus hidup | Tutup jendela = sembunyi ke tray; autostart Windows aktif secara default |
| Toolchain | Rust `x86_64-pc-windows-gnu` + MinGW-w64, seluruhnya di `D:\rust` |
| TLS | rustls (tanpa OpenSSL) |

## 4. Arsitektur

Cargo workspace di akar `open-idm-src`:

```
open-idm-src/
├── Cargo.toml                 # workspace
├── crates/
│   ├── openidm-core/          # logika tanpa UI, bisa diuji sendiri
│   │   ├── db.rs              # SQLite (rusqlite, bundled), skema lama
│   │   ├── engine.rs          # probe, unduh multi-segmen, jeda/lanjut
│   │   ├── queue.rs           # antrean, maks. 3 unduhan bersamaan
│   │   ├── naming.rs          # sanitasi nama file, kategori, nama unik
│   │   ├── settings.rs        # baca/tulis settings.json
│   │   └── protocol.rs        # tipe pesan + framing Native Messaging
│   └── openidm-host/          # openidm-host.exe (dipanggil browser)
├── src-tauri/                 # OpenIDM.exe
│   ├── main.rs                # setup, single instance, tray, autostart
│   ├── commands.rs            # perintah Tauri pengganti /api/*
│   ├── pipe.rs                # server named pipe untuk host
│   └── register.rs            # pendaftaran host di registry HKCU
├── ui/                        # dipakai ulang
│   ├── api.js                 # BARU: adapter invoke Tauri
│   ├── confirm.html/.js       # BARU: dialog konfirmasi
│   └── index.html, app.js, styles.css
└── browser-extension/
```

Batas tanggung jawab:

- `openidm-core` tidak tahu soal Tauri atau browser. Semua aturan unduh ada di sini.
- `openidm-host` hanya meneruskan pesan; tidak punya logika unduh.
- `src-tauri` menghubungkan UI, tray, pipe, dan core.

### 4.1 Proses dan komunikasi

```
Chrome/Edge ──stdin/stdout──> openidm-host.exe ──named pipe──> OpenIDM.exe
 (extension)   Native Messaging                 \\.\pipe\openidm-<user SID>
```

- Browser menjalankan `openidm-host.exe` saat extension memanggil `chrome.runtime.connectNative("com.openidm.host")`.
- Host mencoba menyambung ke pipe. Bila pipe tidak ada, host menjalankan `OpenIDM.exe --background` dari foldernya sendiri dan mencoba ulang sampai 5 detik. Bila tetap gagal, host membalas `{"ok":false,"error":"app_unavailable"}`.
- `OpenIDM.exe` adalah single instance. Peluncuran kedua hanya memunculkan jendela instance pertama.

### 4.2 Protokol pesan

JSON, satu permintaan satu balasan, dikorelasikan dengan `id`.

| `type` | Dari → ke | Isi | Balasan |
|---|---|---|---|
| `ping` | ext → app | — | `{ok, version, capabilities}` |
| `capture` | ext → app | `url, filename, filesize, headers, referrer` | `{ok, decision}`; `decision` = `accepted` \| `later` \| `declined` \| `failed` (+ `error`) |
| `add` | ext → app | sama dengan `capture`, tanpa dialog | `{ok, task_id}` |
| `show` | ext → app | `add_url?` | `{ok}` |

`capture` baru dibalas `accepted` setelah app menerima byte pertama dari server, atau `failed` bila server menolak sebelum itu. `later` berarti tugas disimpan dalam status dijeda di OpenIDM; bagi extension artinya sama dengan `accepted`.

Ukuran pesan mengikuti batas Native Messaging (1 MB ke browser). Header yang diteruskan hanya `Cookie`, `Referer`, `User-Agent`, `Origin`.

## 5. Alur auto-capture

1. `chrome.downloads.onCreated` terpicu. Extension melewati unduhan yang: dimulai extension sendiri, berbahaya menurut browser, bukan http/https, memakai metode selain GET, atau membawa header `Authorization`. Bila auto-capture dimatikan di pengaturan extension, semua dilewati.
2. Extension menjeda unduhan browser dan mengirim `capture`.
3. App memunculkan dialog konfirmasi di depan jendela lain: nama file (bisa diubah), ukuran, kategori, folder simpan.
4. Hasil:
   - **Mulai** → app mengunduh. Setelah byte pertama: balas `accepted`, extension membatalkan unduhan browser.
   - **Nanti** → tugas disimpan dijeda, balas `later`, extension membatalkan unduhan browser.
   - **Batal** atau dialog ditutup → balas `declined`, extension melanjutkan unduhan browser.
   - Server menolak sebelum byte pertama → balas `failed`, extension melanjutkan unduhan browser dan menampilkan notifikasi.
5. Bila app tidak bisa dijalankan, host membalas `app_unavailable` paling lambat 5 detik (§4.1) dan extension melanjutkan unduhan browser. Bila sambungan ke host putus sebelum ada balasan, extension juga melanjutkan unduhan browser. Extension sendiri tidak memasang batas waktu pada `capture`, karena dialog boleh terbuka selama pengguna belum memilih.

Prinsip: satu file tidak pernah diunduh dua kali dan tidak pernah hilang. Extension menyimpan catatan fase di `chrome.storage.local` (seperti sekarang) agar restart service worker tidak membuat unduhan terjeda selamanya; alarm pemulihan 1 menit dipertahankan.

Unduhan yang ditangkap selalu diperlakukan sebagai file langsung. Tidak ada penebakan jenis berdasarkan nama host, sehingga tautan Google Takeout (`takeout-download-drive.usercontent.google.com`) diunduh biasa dengan cookie browser.

## 6. Engine unduh

- **Probe**: `GET` dengan `Range: bytes=0-0`. Dari balasan diambil ukuran total (`Content-Range`), dukungan Range, dan nama file (`Content-Disposition`) bila belum ada.
- **Multi-segmen**: bila server mendukung Range dan ukuran ≥ 2 MB, file dibagi 8 segmen yang diunduh paralel langsung ke posisi masing-masing di satu file `.part` yang sudah dialokasikan.
- **Satu koneksi**: bila tidak mendukung Range, ukuran tidak diketahui, atau < 2 MB.
- **Jeda/lanjut**: posisi tiap segmen disimpan di `<nama>.part.json` di samping file `.part`, diperbarui berkala. Lanjut membaca berkas itu, termasuk setelah app ditutup. Unduhan satu koneksi tanpa Range dimulai ulang dari awal saat dilanjutkan, dan UI menandainya "tidak bisa dilanjutkan".
- **Coba ulang**: tiap segmen dicoba ulang hingga 5 kali dengan jeda bertambah. Bila server mengembalikan 200 untuk permintaan Range di tengah jalan, tugas jatuh ke satu koneksi.
- **Selesai**: `.part` diganti nama ke nama akhir. Bila nama sudah ada, dipakai nama unik `nama (1).ext`. `.part.json` dihapus.
- **Ruang disk**: sebelum mulai, bila ukuran diketahui dan ruang kosong di drive tujuan kurang dari ukuran file + 100 MB, tugas gagal dengan pesan yang menyebut drive dan kekurangannya.
- **Antrean**: maksimal 3 unduhan aktif; sisanya berstatus `pending` dan mulai otomatis.
- **Saat app start**: tugas berstatus `downloading` dari sesi sebelumnya dijadikan `paused`.

Kecepatan dan sisa waktu dihitung dari rata-rata bergerak beberapa detik terakhir.

## 7. Data

- Berkas `downloads.db` dan skemanya sama dengan versi Python, mode WAL. Kolom `request_id` ditambahkan bila belum ada (migrasi yang sama dengan `core/database.py`). App Rust dan core Python tidak boleh menulis berkas yang sama bersamaan; lihat §11.
- Lokasi data: folder yang sama dengan `OpenIDM.exe` (portable), sama seperti sekarang.
- `settings.json` di folder yang sama:

| Kunci | Default |
|---|---|
| `download_dir` | `%USERPROFILE%\Downloads\OpenIDM` |
| `max_concurrent` | 3 |
| `segments` | 8 |
| `autostart` | true |
| `confirm_dialog` | true |

Subfolder kategori (`Video`, `Music`, `Documents`, `Programs`, `Compressed`, `General`) dipertahankan.

Header yang disimpan di kolom `headers` mencakup cookie agar unduhan bisa dilanjutkan. Cookie dihapus dari baris itu saat unduhan selesai atau dihapus.

## 8. UI

- `ui/api.js` menyediakan fungsi yang sama dengan endpoint lama (`status`, `downloads`, `download`, `control`, `openFile`, `openFolder`, `info`) di atas `invoke` Tauri. `app.js` hanya diubah di titik pemanggilan `fetch`.
- Polling 1 detik dipertahankan (lewat `invoke`).
- `info` untuk URL video atau folder Drive mengembalikan pesan jelas bahwa fitur itu belum ada di app ini (Tahap 2/3), bukan error teknis.
- Tombol "matikan server" diganti "Keluar" yang benar-benar menutup app.
- Halaman Pengaturan baru: folder simpan (pemilih folder native), jumlah unduhan bersamaan, autostart, dialog konfirmasi, status integrasi browser dengan tombol "Daftarkan ulang".
- Dialog konfirmasi: jendela Tauri kecil terpisah, selalu di atas, tanpa bingkai browser.
- Tray: Buka OpenIDM, Jeda semua, Lanjutkan semua, Keluar. Klik kiri membuka jendela.
- Mengikuti gaya yang ada (`styles.css`) dan aturan anti-slop proyek: ikon SVG, state hover/active/focus-visible/disabled, empty state, tanpa gradien ungu/pink.

## 9. Perubahan extension

- `manifest.json`: tambah izin `nativeMessaging`; tambah `key` agar ID extension tetap; versi 2.0.0.
- `background.js`: tambah klien Native Messaging (satu port, sambung ulang bila putus, korelasi `id`, batas waktu). Fungsi `status()` memakai `ping`.
- `capture.js`: alur §5 menggantikan POST `/download` + polling `request_id`. `showCapturedDashboard` (membuka tab) dihapus; app sendiri yang tampil.
- `OPEN_DASHBOARD` mengirim `show`, tidak lagi membuka tab.
- Perutean unduhan manual: file langsung → `add` ke app Rust. Halaman video dan Drive (`corePage`) dan stream → tetap ke core Python lewat HTTP seperti sekarang, dengan pesan jelas bila core itu tidak aktif.
- `INSTALL.md` dan `README.md` diperbarui.

## 10. Pendaftaran Native Messaging

Saat start, `OpenIDM.exe` menulis `openidm-host.json` di foldernya (path absolut ke `openidm-host.exe`, `allowed_origins` berisi tepat satu ID extension) dan mendaftarkannya di:

- `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.openidm.host`
- `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.openidm.host`

Karena ditulis ulang setiap start, folder portable yang dipindah tetap berfungsi setelah app dibuka sekali. Autostart memakai `HKCU\...\Run` dengan argumen `--background`. Tidak ada yang memerlukan hak admin.

## 11. Hidup berdampingan dengan core Python (selama Tahap 1–2)

- Di folder rilis `open-idm`, core Python dipindah utuh ke subfolder `legacy\` (`OpenIDM.exe`, `_internal\`, `ui\`, `bin\`) bersama salinan `downloads.db`. Core itu mencari datanya di folder exe-nya sendiri, jadi tidak perlu diubah atau dibuild ulang. App Rust di akar `open-idm` memakai `downloads.db` yang asli.
- Akibatnya unduhan video baru yang dibuat core lama tampil di dashboard lama, bukan di app Rust. Ini batasan Tahap 1 yang diterima dan hilang di Tahap 2.

## 12. Keamanan

- Host manifest hanya mengizinkan satu ID extension.
- Named pipe dibuat dengan ACL hanya untuk SID pengguna yang sedang login, dan menolak klien jarak jauh.
- URL wajib `http`/`https` tanpa kredensial tertanam. Nama file dibersihkan: tanpa pemisah path, karakter terlarang Windows, nama perangkat (`CON`, `NUL`, …), panjang ≤ 180.
- Path akhir selalu diverifikasi berada di dalam `download_dir`.
- Webview: CSP ketat, hanya aset lokal, tanpa navigasi ke luar. Perintah Tauri memvalidasi semua argumen di sisi Rust.
- `open_file` hanya membuka path milik tugas yang ada di database.
- Tidak ada rahasia di kode. Pesan error ke UI tanpa stack trace.

## 13. Penanganan error

| Kondisi | Perilaku |
|---|---|
| App tidak bisa dijalankan host | Unduhan browser dilanjutkan, notifikasi extension |
| Server menolak (403/404/…) sebelum byte pertama | `failed`, unduhan browser dilanjutkan |
| Putus di tengah unduhan | Coba ulang per segmen; bila habis, status `error` dengan tombol Lanjutkan |
| Disk tidak cukup | Status `error`, pesan menyebut drive dan kekurangan |
| File `.part` hilang saat lanjut | Mulai ulang dari awal |
| Extension lama atau host tidak terdaftar | Extension menampilkan petunjuk "Buka OpenIDM sekali untuk mendaftarkan integrasi" |

## 14. Pengujian

- `openidm-core`: test terhadap server HTTP lokal di dalam test — Range didukung, tanpa Range, ukuran tidak diketahui, putus di tengah lalu lanjut, jeda/lanjut lintas "restart", 403 sebelum byte pertama, nama file bentrok, disk tidak cukup (disimulasikan).
- `naming`: sanitasi dan path traversal.
- `protocol`: framing Native Messaging (panjang 4 byte little-endian), pesan rusak, pesan terlalu besar.
- `openidm-host`: test integrasi dengan pipe palsu, termasuk app tidak tersedia.
- Extension: `node --test tests/extension.test.cjs` diperbarui dengan port native tiruan untuk setiap cabang §5.
- Uji nyata sebelum serah terima: klik unduhan di Chrome → dialog → selesai; Batal → kembali ke browser; app tertutup → menyala sendiri; jeda/lanjut; tautan Google Takeout.

## 15. Build dan rilis

- `RUSTUP_HOME=D:\rust\rustup`, `CARGO_HOME=D:\rust\cargo`, `CARGO_TARGET_DIR=D:\rust\target\openidm`, MinGW-w64 di `D:\rust\mingw64`.
- `build-rust.bat` membangun rilis dan menyalin `OpenIDM.exe`, `openidm-host.exe`, dan `browser-extension/` ke `C:\xampp\htdocs\open-idm`.
- Rilis Tahap 1 tidak membawa `node.exe` atau runtime Python untuk app Rust. `bin\ffmpeg.exe` dan berkas core lama tetap ada sampai Tahap 2–3.
- WebView2 Runtime wajib ada di mesin (sudah terpasang di mesin ini; bawaan Windows 11).
