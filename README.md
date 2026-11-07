# OpenIDM (Open-Source Video & File Download Manager)

## Ekstensi Chrome 1.1

Ekstensi kini mendukung unduhan file langsung melalui Chrome tanpa desktop, mode Otomatis/Chrome/Desktop, menu klik kanan, input URL, serta progres dan jeda/lanjut unduhan Chrome. Streaming dan pilihan kualitas video tetap menggunakan mesin desktop. [Panduan pemasangan, izin, dan batasan](browser-extension/INSTALL.md).

**Pasang:** buka `chrome://extensions`, aktifkan **Developer mode**, klik **Load unpacked**, lalu pilih folder `browser-extension`. Setelah memperbarui ekstensi, reload ekstensi dan tab situs. Mesin desktop diperlukan untuk video platform, HLS/DASH, dan folder Drive; tidak semua video dapat diekstrak dan media DRM tidak didukung.

**Paket ZIP:** jalankan `powershell -ExecutionPolicy Bypass -File scripts\package-extension.ps1`. Hasilnya berada di `dist/`.

**Pengujian:** `node --test tests/extension.test.cjs`. Pengujian browser: `node tests/browser-smoke.cjs` (memerlukan Puppeteer dengan Chrome for Testing; menggunakan profil sementara dan server fixture lokal).

Uji API dan unduhan mesin Python asli: `python tests/core-download-smoke.py` (database dan folder unduhan sementara).

OpenIDM adalah alternatif open-source dari **Internet Download Manager (IDM)** untuk mendeteksi (*sniffing*) media dan mengunduh file dari browser. Dukungan video platform dan streaming mengikuti kemampuan mesin `yt-dlp` yang terpasang; media DRM tidak didukung.

---

## 🚀 Fitur Unggulan

1. **Floating "Download this video" Button (Mirip IDM)**:
   - Tombol mengambang otomatis disematkan di pojok kanan atas video player web.
   - Pilihan resolusi dinamis (*1080p Full HD*, *720p HD*, *480p*, *360p*, dan *Audio Only MP3/M4A*).
   - Indikator ukuran file perkiraan sebelum mengunduh.
2. **Multi-Segment Turbo Downloader**:
   - Memecah berkas langsung menjadi 8 koneksi paralel (`Range: bytes`) untuk memaksimalkan kecepatan bandwidth internet Anda.
   - Otomatis menggabungkan pecahan berkas saat unduhan selesai.
   - Mendukung jeda (*pause*) dan lanjut (*resume*).
3. **Stream Extractor Engine (`yt-dlp` Core)**:
   - Mengekstrak stream yang didukung dan menggabungkan video track + audio track menggunakan FFmpeg.
4. **Desktop Control Dashboard**:
   - Antarmuka web modern dengan gaya *Swiss Minimalist* tanpa AI slop.
   - Kategori otomatis: *Video*, *Musik*, *Dokumen*, *Program*, *Arsip/ZIP*.
   - Filter status unduhan (*Sedang Berjalan*, *Selesai*, *Dijeda*, *Error*).
   - Pengukur kecepatan total real-time (*KB/s* atau *MB/s*).
   - Pintasan langsung membuka file atau folder penyimpanan di Windows Explorer.
5. **Google Drive Folder & File Recognizer (Turbo Multi-Segment)**:
   - Mengenali tautan **Folder Google Drive** secara otomatis tanpa perlu Google Cloud API Key.
   - Menampilkan seluruh berkas di dalam folder secara instan beserta ukuran dan preview thumbnail.
   - Fitur **Batch Download**: unduh seluruh berkas folder sekaligus atau pilih berkas tertentu dengan checkbox.
   - Mendukung tautan berkas tunggal Google Drive dengan bypass scan virus dan multi-koneksi 8-thread paralel.

---

## 📂 Struktur Proyek

```
open-idm/
├── browser-extension/         # Ekstensi Browser (Manifest V3)
│   ├── manifest.json          # Konfigurasi ekstensi Chrome/Edge
│   ├── background.js          # Service worker sniffer jaringan media
│   ├── content.js             # Injector tombol "Download this video" & Google Drive
│   ├── content.css            # Styling tombol floating & menu resolusi
│   ├── popup.html/.js/.css    # Panel toolbar ekstensi
│   └── icons/                 # Ikon ekstensi (16x16, 48x48, 128x128)
├── core/                      # Engine Backend & Downloader
│   ├── config.py              # Konfigurasi folder & port lokal (6899)
│   ├── database.py            # SQLite database manager (WAL mode)
│   ├── gdrive_handler.py      # Google Drive folder parser & direct resolver
│   ├── stream_extractor.py    # Parser video URL & resolusi
│   ├── segmented_downloader.py# Multi-connection download engine
│   ├── task_manager.py        # Antrean unduhan & batch thread controller
│   └── server.py              # REST API server & static UI host
├── ui/                        # Antarmuka Desktop Dashboard
│   ├── index.html             # Tampilan utama
│   ├── styles.css             # Tema dark Swiss Minimalist
│   └── app.js                 # Logika real-time update & interaksi
├── main.py                    # Entry point aplikasi
├── run.bat                    # Peluncur 1-Click untuk Windows
└── README.md                  # Panduan dokumentasi
```

---

## 🛠️ Cara Penggunaan

### Langkah 1: Jalankan OpenIDM Desktop

Cukup **klik dua kali** file:
```
run.bat
```
*(Atau jalankan `python main.py` di terminal).*

Browser akan otomatis membuka halaman dashboard OpenIDM di `http://127.0.0.1:6899`.

---

### Langkah 2: Pasang Ekstensi di Browser (Hanya Sekali)

1. Buka browser Anda (**Google Chrome**, **Microsoft Edge**, **Brave**, atau **Opera**).
2. Ketik di bilah alamat:
   - Chrome/Brave: `chrome://extensions`
   - Edge: `edge://extensions`
3. Aktifkan tombol toggle **"Developer mode"** di pojok kanan atas.
4. Klik tombol **"Load unpacked"** (Muat yang belum dibongkar).
5. Pilih folder:
   ```
   c:\xampp\htdocs\open-idm\browser-extension
   ```
6. Ekstensi **OpenIDM — Download Manager** kini telah aktif di browser Anda!

---

### Langkah 3: Mengunduh Video dari Web

1. Buka situs video apa saja (misalnya YouTube).
2. Putar video yang diinginkan.
3. Anda akan melihat tombol **"Download this video"** di pojok kanan atas video player.
4. Klik tombol tersebut, pilih resolusi yang diinginkan (misal: *1080p* atau *720p*).
5. Unduhan akan otomatis terkirim dan berjalan di OpenIDM dengan kecepatan maksimal!
6. Semua file yang selesai diunduh tersimpan rapi di:
   ```
   C:\Users\<Nama_User>\Downloads\OpenIDM\
   ```

---

### 📦 Mode Standalone Portable (.EXE Tanpa Ketergantungan Python)

Anda dapat menjalankan OpenIDM di komputer Windows mana pun **tanpa perlu menginstall Python, pip, atau Node.js**:

1. Buka folder dist/OpenIDM/.
2. Klik 2x file OpenIDM.exe.
3. Aplikasi akan berjalan di latar belakang (tanpa jendela hitam CMD) dan langsung membuka Dashboard Web di browser default Anda.
4. Untuk mematikan aplikasi, klik tombol ikon merah di pojok kanan atas Dashboard atau jalankan Hentikan-OpenIDM.bat.
5. Untuk membangun ulang executable setelah ada perubahan kode, cukup klik 2x build.bat.

#### 💻 Opsi Baris Perintah (CLI Options)
`ash
python main.py --help         # Menampilkan bantuan dan seluruh opsi perintah
python main.py --status       # Memeriksa apakah OpenIDM Core sedang aktif
python main.py --stop         # Menghentikan server OpenIDM yang sedang berjalan
python main.py --restart      # Memaksa restart & membersihkan port dari zombie
python main.py --port 6900    # Menjalankan server pada port kustom
python main.py --no-browser   # Menjalankan server tanpa otomatis membuka browser
`
