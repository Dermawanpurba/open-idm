import os
import sys
import time
import argparse
import webbrowser
import threading
from core.config import SERVER_HOST, SERVER_PORT, DOWNLOADS_DIR
from core.server import run_server, check_existing_instance, kill_process_on_port

# Safely handle windowless mode (--noconsole) where stdout/stderr are None
if sys.stdout is None:
    try:
        sys.stdout = open(os.devnull, 'w', encoding='utf-8', errors='replace')
    except Exception:
        pass
if sys.stderr is None:
    try:
        sys.stderr = open(os.devnull, 'w', encoding='utf-8', errors='replace')
    except Exception:
        pass

def print_banner(host=SERVER_HOST, port=SERVER_PORT):
    print("=" * 65)
    print("           OPENIDM - Open Source Video & File Downloader           ")
    print("=" * 65)
    print(f" [*] Dashboard Web      : http://{host}:{port}")
    print(f" [*] Folder Unduhan     : {DOWNLOADS_DIR}")
    print(f" [*] Engine             : Multi-Segment (8-conn) + yt-dlp Core")
    print(f" [*] Ekstensi Browser   : open-idm/browser-extension")
    print("=" * 65)
    print("Tekan Ctrl+C di terminal ini untuk menghentikan aplikasi.\n")

def open_browser(host=SERVER_HOST, port=SERVER_PORT):
    time.sleep(1.2)
    url = f"http://{host}:{port}"
    try:
        webbrowser.open(url)
    except Exception:
        pass

def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace', line_buffering=True)

    parser = argparse.ArgumentParser(
        description="OpenIDM - Open-Source Video & File Download Manager",
        formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument("--port", "-p", type=int, default=SERVER_PORT, help="Port server HTTP (default: 6899)")
    parser.add_argument("--host", type=str, default=SERVER_HOST, help="Host address bind (default: 127.0.0.1)")
    parser.add_argument("--no-browser", action="store_true", help="Jangan otomatis membuka browser saat server mulai")
    parser.add_argument("--restart", "--force", action="store_true", help="Paksa hentikan proses lama di port dan mulai ulang")
    parser.add_argument("--status", action="store_true", help="Cek apakah OpenIDM Core sedang aktif")
    parser.add_argument("--stop", "--shutdown", action="store_true", help="Hentikan server OpenIDM yang sedang berjalan")
    parser.add_argument("-v", "--version", action="version", version="OpenIDM Core v1.1.0")

    args = parser.parse_args()

    # Handle --status
    if args.status:
        existing = check_existing_instance(args.host, args.port)
        if existing:
            print(f"[OK] OpenIDM Core aktif di http://{args.host}:{args.port}")
            print(f"     Status: {existing.get('status')}, Unduhan aktif: {existing.get('active_downloads', 0)}")
            sys.exit(0)
        else:
            print(f"[OFFLINE] OpenIDM Core tidak berjalan di http://{args.host}:{args.port}")
            sys.exit(1)

    # Handle --stop
    if args.stop:
        import urllib.request
        try:
            req = urllib.request.Request(f"http://{args.host}:{args.port}/api/shutdown", data=b"{}", headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(req, timeout=3):
                pass
            print(f"[*] Perintah shutdown terkirim ke http://{args.host}:{args.port}")
        except Exception:
            pass
        killed = kill_process_on_port(args.port)
        if killed:
            print(f"[*] Proses di port {args.port} berhasil dihentikan.")
        else:
            print("[*] OpenIDM telah berhenti.")
        sys.exit(0)

    # Handle --restart / --force
    if args.restart:
        print(f"[*] Menghentikan instance/proses lama di port {args.port}...")
        kill_process_on_port(args.port)
        time.sleep(0.5)
    else:
        # Check if an existing OpenIDM instance is already healthy and running
        existing = check_existing_instance(args.host, args.port)
        if existing:
            print(f"[*] OpenIDM Core sudah berjalan di http://{args.host}:{args.port}")
            if not args.no_browser:
                open_browser(args.host, args.port)
            sys.exit(0)

    print_banner(args.host, args.port)

    # Open dashboard in browser automatically
    if not args.no_browser:
        threading.Thread(target=open_browser, args=(args.host, args.port), daemon=True).start()

    # Run core server
    run_server(host=args.host, port=args.port)

if __name__ == "__main__":
    main()
