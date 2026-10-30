OPENIDM - PANDUAN INSTALASI EKSTENSI CHROME
===========================================

Repositori:
https://github.com/Dermawanpurba/open-idm

1. Ambil kode OpenIDM
---------------------

Cara clone pertama kali:

    git clone https://github.com/Dermawanpurba/open-idm.git
    cd open-idm

Jika folder open-idm sudah ada:

    git pull origin main

2. Pasang ekstensi Chrome
-------------------------

1. Buka Google Chrome.
2. Buka alamat chrome://extensions.
3. Aktifkan Developer mode / Mode pengembang.
4. Klik Load unpacked / Muat yang belum dibongkar.
5. Pilih folder:

       C:\xampp\htdocs\open-idm\browser-extension

   Pilih folder yang berisi manifest.json.
6. Sematkan OpenIDM - Download Manager dari menu Extensions Chrome.
7. Muat ulang tab website yang sudah terbuka.

Chrome minimal: versi 109.

3. Pilih mode unduhan
---------------------

Popup ekstensi menyediakan tiga mode:

- Otomatis: gunakan OpenIDM Desktop jika aktif; file langsung memakai Chrome
  jika desktop tidak aktif.
- Chrome: unduh file langsung melalui Chrome tanpa menjalankan desktop.
- OpenIDM Desktop: gunakan mesin Python untuk video platform, pilihan kualitas,
  HLS/DASH, penggabungan audio-video, dan Google Drive.

Untuk video streaming, jalankan run.bat di folder open-idm terlebih dahulu.
Dashboard desktop tersedia di http://127.0.0.1:6899.

4. Cara menggunakan ekstensi
----------------------------

- Tempel URL file ke popup, lalu klik Unduh.
- Klik kanan link, video, atau halaman, lalu pilih Unduh dengan OpenIDM.
- Putar video, lalu gunakan tombol Download this video.
- Gunakan Kirim halaman untuk halaman YouTube, platform video, atau Google Drive.
- File langsung disimpan melalui Chrome. Pilihan kualitas dan folder Drive
  diproses oleh OpenIDM Desktop.

Media DRM tidak didukung. Tidak semua website dapat diekstrak oleh yt-dlp.

5. Update kode dan ekstensi
---------------------------

Masuk ke folder proyek, lalu jalankan:

    git pull origin main

Jika ekstensi sudah terpasang sebagai Load unpacked:

1. Buka chrome://extensions.
2. Klik tombol Reload pada kartu OpenIDM.
3. Muat ulang tab website.

Jika ingin membuat paket ZIP:

    powershell -ExecutionPolicy Bypass -File scripts\package-extension.ps1

Hasil paket ada di:

    dist\openidm-chrome
    dist\openidm-chrome-1.1.0.zip

6. Pengembangan dan pengujian
-----------------------------

Uji logika ekstensi:

    node --test tests\extension.test.cjs

Uji di Chrome:

    node tests\browser-smoke.cjs

Uji API dan downloader Python:

    python tests\core-download-smoke.py

Panduan lengkap, izin, batasan, dan troubleshooting:

    browser-extension\INSTALL.md

