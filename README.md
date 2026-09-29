# OpenIDM (Open-Source Video & File Download Manager)

OpenIDM adalah alternatif open-source dari **Internet Download Manager (IDM)** yang dilengkapi kemampuan mendeteksi (*sniffing*) dan mengunduh video apa pun yang sedang diputar di browser web (YouTube, TikTok, Twitter/X, Instagram, Facebook, Vimeo, hingga streaming HTML5).

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
   - Mampu mengekstrak stream video terenkripsi atau berpisah (video track + audio track) pada platform modern.
4. **Desktop Control Dashboard**:
   - Antarmuka web modern dengan gaya *Swiss Minimalist* tanpa AI slop.
   - Kategori otomatis: *Video*, *Musik*, *Dokumen*, *Program*, *Arsip/ZIP*.
   - Filter status unduhan (*Sedang Berjalan*, *Selesai*, *Dijeda*, *Error*).
   - Pengukur kecepatan total real-time (*KB/s* atau *MB/s*).
   - Pintasan langsung membuka file atau folder penyimpanan di Windows Explorer.

---

## 📂 Struktur Proyek

```
open-idm/
├── browser-extension/         # Ekstensi Browser (Manifest V3)
│   ├── manifest.json          # Konfigurasi ekstensi Chrome/Edge
│   ├── background.js          # Service worker sniffer jaringan media
│   ├── content.js             # Injector tombol "Download this video"
│   ├── content.css            # Styling tombol floating & menu resolusi
│   ├── popup.html/.js/.css    # Panel toolbar ekstensi
│   └── icons/                 # Ikon ekstensi (16x16, 48x48, 128x128)
├── core/                      # Engine Backend & Downloader
│   ├── config.py              # Konfigurasi folder & port lokal (6899)
│   ├── database.py            # SQLite database manager (WAL mode)
│   ├── stream_extractor.py    # Parser video URL & resolusi
│   ├── segmented_downloader.py# Multi-connection download engine
│   ├── task_manager.py        # Antrean unduhan & thread controller
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
6. Ekstensi **OpenIDM Video Downloader** kini telah aktif di browser Anda!

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
