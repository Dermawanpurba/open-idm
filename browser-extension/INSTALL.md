# OpenIDM portable: Chrome, Edge, dan Brave

1. Jalankan `OpenIDM.exe` dari folder portable dan biarkan tetap aktif.
2. Buka `chrome://extensions`, `edge://extensions`, atau `brave://extensions`.
3. Aktifkan **Developer mode**, pilih **Load unpacked**, lalu pilih subfolder `browser-extension` dalam paket ini.
4. Jika sudah terpasang, klik **Reload** pada kartu ekstensi setelah pembaruan.
5. Muat ulang tab situs. Pada popup OpenIDM pilih **Otomatis** atau **OpenIDM Desktop**.

## Download biasa dari website

Klik tombol Download website seperti biasa. Ekstensi menangkap unduhan HTTP/HTTPS yang benar-benar dimulai, termasuk endpoint tanpa ekstensi nama berkas. Unduhan browser dijeda, lalu dibatalkan hanya setelah desktop mulai menerima data. Dashboard desktop akan dibuka atau diaktifkan.

Jika desktop belum aktif, unduhan dilanjutkan di browser. Mode **Chrome** menonaktifkan pengalihan otomatis. Unduhan dari ekstensi lain tidak diambil alih. Unduhan `blob:`, POST, atau Authorization khusus tetap memakai sesi browser. Jika desktop menerima halaman login atau gagal mengunduh, salinan browser dilanjutkan.

Jika konfirmasi ke desktop terputus, unduhan browser tetap dijeda untuk mencegah salinan ganda. Periksa Dashboard sebelum melanjutkannya secara manual. Ekstensi memeriksa ulang pengalihan yang tertunda setiap menit dan setelah service worker dimuat ulang; permintaan yang sama tidak ditambahkan dua kali.

## Video dan Google Drive

Gunakan **Download this video**, media terdeteksi pada popup, atau klik kanan **Unduh dengan OpenIDM**. Untuk YouTube/platform video dan folder Google Drive, gunakan **Kirim halaman** atau **Buka di OpenIDM Desktop**. Pada Drive, buka subfolder lalu pilih berkas.

FFmpeg dan Node sudah disertakan dalam paket portable. Dukungan situs mengikuti mesin yt-dlp; tidak semua video bisa diekstrak dan DRM tidak didukung. Ekstensi tidak menyalin cookie atau token Authorization ke desktop. Situs yang memerlukan sesi login khusus dapat tetap memerlukan browser.

## Penyimpanan dan pemindahan folder

Unduhan desktop baru berada pada subfolder `downloads` dalam paket. Riwayat baru menggunakan lokasi relatif sehingga tetap terbaca saat seluruh folder dipindah. Berkas unduhan lama di luar paket tetap berada di lokasi lamanya dan tidak dipindahkan otomatis.

Hentikan OpenIDM terlebih dahulu, lalu salin seluruh folder aplikasi, termasuk `_internal`, `bin`, `ui`, `browser-extension`, `data`, `downloads`, dan `downloads.db` bila ada. Setelah lokasi dipindah, muat ulang ekstensi melalui **Load unpacked** dari lokasi baru. Tidak perlu Python, pip, Node sistem, instalasi, atau perubahan registry.

EXE harus sudah berjalan agar pengalihan otomatis bekerja. Ekstensi portable tidak dapat menyalakan EXE yang belum berjalan tanpa memasang launcher Windows. Menutup dashboard tidak menghentikan mesin; gunakan tombol Matikan atau `Hentikan-OpenIDM.bat` untuk menghentikannya.

## Izin dan data

- HTTP/HTTPS dan `webRequest`: mengenali respons media dan metadata unduhan.
- `downloads`: menjeda, melanjutkan, dan membatalkan salinan browser saat pengalihan.
- `storage`: mode unduhan, daftar media sementara, dan catatan pemulihan pengalihan.
- `alarms`: memeriksa ulang pengalihan tertunda.
- `activeTab`, `contextMenus`, dan `notifications`: popup, menu klik kanan, dan pemberitahuan.

Tidak ada telemetri. Desktop dihubungi melalui `127.0.0.1:6899`. Hanya Referer, Origin, dan User-Agent untuk URL yang sama yang dapat diteruskan; cookie dan Authorization tidak disimpan. Halaman internal browser dan Chrome Web Store tidak dapat dipindai.
