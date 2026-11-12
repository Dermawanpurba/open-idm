import os
import sys
import shutil
import subprocess
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent
DIST_DIR = BASE_DIR / "dist"
BUILD_DIR = BASE_DIR / "build"
OUTPUT_APP_DIR = DIST_DIR / "OpenIDM"
ICON_PATH = BASE_DIR / "icon.ico"

def patch_server_shutdown():
    server_path = BASE_DIR / "core" / "server.py"
    content = server_path.read_text(encoding="utf-8")
    if "/api/shutdown" in content:
        return

    marker = 'success = task_manager.open_folder(task_id)\n                self._send_json({"success": success})\n                return'
    crlf_marker = marker.replace('\n', '\r\n')
    
    shutdown_code = (
        '\n\n            if path == "/api/shutdown":\n'
        '                self._send_json({"success": True, "message": "Server shutting down..."})\n'
        '                import threading, time\n'
        '                def _do_shutdown():\n'
        '                    time.sleep(0.5)\n'
        '                    os._exit(0)\n'
        '                threading.Thread(target=_do_shutdown, daemon=True).start()\n'
        '                return'
    )
    
    if crlf_marker in content:
        content = content.replace(crlf_marker, crlf_marker + shutdown_code.replace('\n', '\r\n'), 1)
        server_path.write_text(content, encoding="utf-8")
        print("[*] Added /api/shutdown endpoint to core/server.py")
    elif marker in content:
        content = content.replace(marker, marker + shutdown_code, 1)
        server_path.write_text(content, encoding="utf-8")
        print("[*] Added /api/shutdown endpoint to core/server.py")
    else:
        print("[!] Warning: Could not auto-patch /api/shutdown in core/server.py")

def build_exe():
    print("=" * 65)
    print("        OpenIDM - Standalone Portable EXE Builder")
    print("=" * 65)

    # 0. Hentikan OpenIDM.exe jika sedang berjalan agar file tidak terkunci oleh Windows
    if sys.platform == "win32":
        subprocess.run("taskkill /F /IM OpenIDM.exe", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        import time
        time.sleep(1.0)

    patch_server_shutdown()

    # 1. Pastikan icon.ico ada
    if not ICON_PATH.exists():
        from PIL import Image
        png_path = BASE_DIR / "browser-extension" / "icons" / "icon128.png"
        img = Image.open(png_path)
        img.save(ICON_PATH, sizes=[(16,16), (32,32), (48,48), (64,64), (128,128)])
        print("[*] Generated icon.ico")

    # 2. Siapkan command PyInstaller
    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--noconfirm",
        "--onedir",
        "--windowed",
        "--name", "OpenIDM",
        f"--icon={ICON_PATH}",
        "--collect-all", "yt_dlp",
        "--hidden-import", "requests",
        "--hidden-import", "sqlite3",
        "--hidden-import", "imageio_ffmpeg",
        f"--distpath={DIST_DIR}",
        f"--workpath={BUILD_DIR}",
        str(BASE_DIR / "main.py")
    ]

    print("[*] Menjalankan PyInstaller (ini mungkin membutuhkan waktu 1-2 menit)...")
    result = subprocess.run(cmd)
    if result.returncode != 0:
        print(f"[ERROR] PyInstaller gagal dengan exit code {result.returncode}")
        sys.exit(result.returncode)

    print("[*] PyInstaller selesai meng-compile.")

    # 3. Salin folder ui/ ke dist/OpenIDM/ui
    target_ui = OUTPUT_APP_DIR / "ui"
    if target_ui.exists():
        shutil.rmtree(target_ui)
    shutil.copytree(BASE_DIR / "ui", target_ui)
    print(f"[*] Berhasil menyalin antarmuka UI ke {target_ui}")

    # 4. Salin folder bin/ (ffmpeg.exe) ke dist/OpenIDM/bin
    target_bin = OUTPUT_APP_DIR / "bin"
    target_bin.mkdir(parents=True, exist_ok=True)
    ffmpeg_src = BASE_DIR / "bin" / "ffmpeg.exe"
    if ffmpeg_src.exists():
        shutil.copy2(ffmpeg_src, target_bin / "ffmpeg.exe")
        print(f"[*] Berhasil menyalin bin/ffmpeg.exe ke {target_bin}")

    # 5. Salin browser-extension ke dist/OpenIDM/browser-extension
    target_ext = OUTPUT_APP_DIR / "browser-extension"
    if target_ext.exists():
        shutil.rmtree(target_ext)
    shutil.copytree(BASE_DIR / "browser-extension", target_ext)
    print(f"[*] Berhasil menyalin browser-extension ke {target_ext}")

    # 6. Buat skrip pembantu Hentikan-OpenIDM.bat
    stop_script = (
        "@echo off\r\n"
        "chcp 65001 >nul\r\n"
        "echo Menghentikan proses OpenIDM.exe...\r\n"
        "taskkill /F /IM OpenIDM.exe >nul 2>&1\r\n"
        "echo OpenIDM berhasil dihentikan.\r\n"
        "timeout /t 2 >nul\r\n"
    )
    (OUTPUT_APP_DIR / "Hentikan-OpenIDM.bat").write_text(stop_script, encoding="utf-8")

    # 7. Buat instruksi ringkas CARA-PAKAI.txt
    readme_txt = (
        "=================================================================\r\n"
        "                    OpenIDM Standalone Portable                  \r\n"
        "=================================================================\r\n\r\n"
        "1. CARA MENJALANKAN:\r\n"
        "   - Cukup klik 2x pada berkas 'OpenIDM.exe'.\r\n"
        "   - Aplikasi akan otomatis berjalan di latar belakang (tanpa jendela CMD)\r\n"
        "     dan langsung membuka Dashboard Web di browser default Anda.\r\n\r\n"
        "2. CARA PASANG EKSTENSI CHROME / EDGE:\r\n"
        "   - Buka chrome://extensions (atau edge://extensions)\r\n"
        "   - Aktifkan 'Developer mode' (Mode pengembang) di pojok kanan atas.\r\n"
        "   - Klik tombol 'Load unpacked' (Muat yang belum dibongkar).\r\n"
        "   - Pilih folder 'browser-extension' di dalam folder ini.\r\n\r\n"
        "3. CARA MENGHENTIKAN APLIKASI:\r\n"
        "   - Cukup klik 2x 'Hentikan-OpenIDM.bat' atau tutup lewat tombol Matikan di Web Dashboard.\r\n\r\n"
        "Catatan: Anda TIDAK MEMERLUKAN Python, pip, atau Node.js untuk menjalankan folder ini.\r\n"
        "Folder ini 100% portable dan mandiri.\r\n"
    )
    (OUTPUT_APP_DIR / "CARA-PAKAI.txt").write_text(readme_txt, encoding="utf-8")

    print("=" * 65)
    print(" [SUKSES] Paket OpenIDM Standalone Portable Portable Selesai!")
    print(f" [Lokasi] {OUTPUT_APP_DIR}")
    print("=" * 65)

if __name__ == "__main__":
    build_exe()

