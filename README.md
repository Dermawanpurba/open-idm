# OpenIDM 2.0 — portable Rust

Pengembangan aktif memakai **Rust + Tauri + SQLite**. Paket portable Python sudah dihentikan; jalur build dan peluncur utama sekarang memakai Rust.

## Folder utama

| Lokasi | Fungsi |
|---|---|
| `C:\xampp\htdocs\open-idm-src` | Kode sumber dan pengujian Rust. |
| `C:\xampp\htdocs\open-idm` | Paket portable Rust utama. |
| `D:\rust\run-test` | Paket/data uji terpisah; bukan tujuan rilis utama. |

## Build dan jalankan

Jalankan `build-rust.bat` atau `build.bat`. Skrip menjalankan tes Rust dan ekstensi, membangun mode release, lalu menyalin paket ke folder `open-idm` di sebelah sumber. Lokasi keluaran lain dapat diberikan sebagai argumen pertama.

Toolchain pengembangan berada di `D:\rust`; ubah `RUST_ROOT` bila toolchain dipindah. Pengguna paket hasil build tidak memerlukan Rust, Cargo, Node, atau Python. Windows memerlukan WebView2 Runtime untuk tampilan Tauri.

Jalankan paket melalui `open-idm\OpenIDM.exe`, `Jalankan-OpenIDM.bat`, atau `run.bat` dari folder sumber. Tutup paket tujuan melalui tray sebelum membangunnya ulang. Instance uji dari folder lain tidak dihentikan oleh skrip build.

## Isi paket

- `OpenIDM.exe`: aplikasi Rust/Tauri dengan UI tertanam.
- `openidm-host.exe`: penghubung Native Messaging.
- `WebView2Loader.dll`: loader WebView2 di samping EXE.
- `browser-extension`: ekstensi 2.0 untuk Chrome/Edge.
- `bin`: helper video portable `yt-dlp.exe`, FFmpeg, dan Node. Kualitas YouTube dianalisis dan unduhan dikelola antrean Rust; tidak memakai server Python legacy.
- `downloads.db` dan `settings.json`: data yang dibuat saat aplikasi digunakan.

Lihat [panduan portable](docs/portable-rust.txt) dan [pemasangan ekstensi](browser-extension/INSTALL.md). Data tersimpan di samping EXE; lokasi berkas unduhan mengikuti Pengaturan. Native Messaging dan opsi autostart memakai pengaturan pengguna Windows. Tidak ada server Python di port 6899.

## Struktur pengembangan

- `crates/openidm-core`: mesin unduhan, antrean, SQLite, pengaturan, dan protokol.
- `crates/openidm-host`: relay Native Messaging ke aplikasi.
- `src-tauri`: aplikasi desktop, tray, dialog konfirmasi, dan registrasi browser.
- `ui`: antarmuka yang ditanam ke EXE saat build.
- `browser-extension`: integrasi browser.
- `tests`: pengujian ekstensi; tes Rust berada di crate masing-masing.

File sumber Python lama masih disimpan sebagai referensi migrasi; tidak dipakai oleh `build.bat`, `build-rust.bat`, `run.bat`, atau paket portable utama. Cadangan data pengguna dari paket lama berada di `.backups/` dan tidak masuk Git maupun distribusi.
