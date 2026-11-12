# Ekstensi OpenIDM 2.0 untuk paket Rust

1. Jalankan `OpenIDM.exe` dari paket utama. Pastikan `openidm-host.exe` dan `WebView2Loader.dll` berada di sampingnya.
2. Periksa status integrasi browser pada **Pengaturan** aplikasi. Aplikasi mendaftarkan Native Messaging untuk Chrome/Edge pada akun Windows saat ini ketika mulai berjalan.
3. Buka `chrome://extensions` atau `edge://extensions`, aktifkan **Developer mode**, pilih **Load unpacked**, lalu pilih subfolder `browser-extension` dalam paket Rust.
4. Setelah pembaruan, reload ekstensi dan tab situs. Jika lokasi paket dipindah, jalankan aplikasi dari lokasi baru dan muat ekstensi dari lokasi baru.

Versi ini memakai **Native Messaging + named pipe Rust**, bukan server HTTP Python di `127.0.0.1:6899`. Analisis YouTube dan pilihan kualitas juga melalui Rust; helper video portable berada dalam `bin` paket aplikasi. Host browser dan opsi autostart memakai pengaturan pengguna Windows; tidak memerlukan administrator.

Pengalihan unduhan dan dialog konfirmasinya mengikuti pengaturan aplikasi. Unduhan yang tidak dapat dialihkan tetap perlu ditangani browser. Jangan memakai ekstensi paket Python lama dengan paket Rust.

Untuk memindahkan paket, keluar dari aplikasi melalui tray, lalu salin seluruh folder aplikasi beserta database dan pengaturannya. Berkas unduhan di lokasi luar paket harus dibawa secara terpisah. Windows memerlukan WebView2 Runtime untuk tampilan aplikasi.
