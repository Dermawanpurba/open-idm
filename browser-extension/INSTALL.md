# Pasang OpenIDM di Google Chrome

1. Buka `chrome://extensions` di Google Chrome (versi 109 atau lebih baru).
2. Aktifkan **Developer mode** di kanan atas.
3. Klik **Load unpacked / Muat yang belum dibongkar**.
4. Pilih folder `C:\xampp\htdocs\open-idm\browser-extension`.
   Jika memakai ZIP, ekstrak ZIP lebih dahulu, lalu pilih folder berisi `manifest.json`.
5. Klik ikon puzzle Chrome lalu sematkan **OpenIDM — Download Manager**.
6. Muat ulang tab yang sudah terbuka sebelum pemasangan atau pembaruan ekstensi.

## Pilih cara mengunduh

- **Otomatis** (bawaan): kirim ke OpenIDM Desktop jika aktif. Jika desktop belum aktif, file langsung memakai pengunduh Chrome.
- **Chrome**: file langsung memakai Chrome tanpa Python maupun OpenIDM Desktop.
- **OpenIDM Desktop**: semua unduhan memakai mesin Python. Jalankan `run.bat` di folder proyek. Dashboard tersedia di `http://127.0.0.1:6899`.

Tempel tautan ZIP, PDF, MP4, MP3, dan file langsung lain di popup lalu klik **Unduh**. Anda juga bisa klik kanan tautan/video dan pilih **Unduh dengan OpenIDM**. Progres, jeda/lanjut (jika server mendukung), serta tombol folder tersedia untuk unduhan Chrome yang dimulai oleh ekstensi ini. **Lihat semua** membuka pengelola unduhan Chrome; unduhan desktop dipantau melalui **Dashboard desktop**.

Opsi **Tanyakan lokasi penyimpanan** berlaku untuk unduhan Chrome. Jika dimatikan, Chrome memakai folder unduhan bawaan dan nama dari server. Jika pilihan kualitas video menghasilkan nama khusus, file Chrome diletakkan pada subfolder `OpenIDM`. Desktop tetap memakai `Downloads\OpenIDM`.

## Video, streaming, dan Google Drive

Putar video, lalu gunakan tombol **Download this video** atau buka popup dan klik **Periksa lagi**. Untuk halaman video YouTube/platform sosial dan Google Drive, pilih **Kirim halaman**. Tombol Drive juga dapat membuka tautan di dashboard agar Anda dapat memilih isi folder.

Pilihan kualitas, penggabungan video+audio, HLS/DASH (`.m3u8`, `.mpd`), serta folder Drive memerlukan desktop, `yt-dlp`, dan FFmpeg untuk format yang perlu digabung. Dukungan situs mengikuti kemampuan mesin yang terpasang; tidak semua situs/video dapat diekstrak. Media DRM tidak didukung. Media yang memerlukan sesi login belum tentu berhasil karena ekstensi tidak menyalin cookie atau token Authorization ke desktop.

## Izin dan data

- Akses situs HTTP/HTTPS dan `webRequest`: mendeteksi respons video/audio dan manifest. Potongan `.ts`/`.m4s` tidak ditampilkan sebagai video utuh.
- `activeTab`: membaca alamat/judul tab ketika popup dipakai.
- `downloads`: mengunduh dan mengatur file Chrome yang dimulai ekstensi.
- `storage`: menyimpan mode, opsi lokasi, dan paling banyak 30 ID unduhan. Daftar media dibatasi 60 per tab, disimpan dalam memori sesi, dihapus ketika tab berganti alamat/ditutup atau browser ditutup.
- `contextMenus` dan `notifications`: menu klik kanan dan pemberitahuan hasilnya.

Ekstensi tidak mengirim telemetri. Permintaan ke desktop hanya menuju `127.0.0.1:6899`. Referer, Origin, dan User-Agent media dapat diteruskan untuk URL media yang sama; cookie dan Authorization tidak disimpan. Ekstensi tidak dapat memindai halaman internal seperti `chrome://`, Chrome Web Store, atau media `blob:` secara langsung.

## Jika belum berjalan

- **Desktop belum aktif**: jalankan `run.bat`, lalu klik **Periksa lagi**. File langsung tetap bisa memakai Chrome.
- **Belum ada media**: muat ulang halaman setelah memasang ekstensi, lalu putar videonya. Coba **Kirim halaman** untuk platform video.
- **Tautan tanpa ekstensi file**: gunakan media yang terdeteksi dari respons server atau kirim ke desktop agar dianalisis.
- **Ekstensi diperbarui**: klik tombol reload pada kartu ekstensi di `chrome://extensions`, lalu muat ulang tab situs.

## Membuat paket

Dari folder proyek, jalankan:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-extension.ps1
```

Hasil: `dist\openidm-chrome` (Load unpacked) dan `dist\openidm-chrome-1.1.0.zip` (distribusi manual). Skrip hanya menyalin aset ekstensi, tanpa Python, database, atau binari FFmpeg. Paket ini belum dipublikasikan ke Chrome Web Store.
