import sys
import time
import webbrowser
import threading
from core.config import SERVER_HOST, SERVER_PORT, DOWNLOADS_DIR
from core.server import run_server

def print_banner():
    print("=" * 65)
    print("           OPENIDM - Open Source Video & File Downloader           ")
    print("=" * 65)
    print(f" [*] Dashboard Web      : http://{SERVER_HOST}:{SERVER_PORT}")
    print(f" [*] Folder Unduhan     : {DOWNLOADS_DIR}")
    print(f" [*] Engine             : Multi-Segment (8-conn) + yt-dlp Core")
    print(f" [*] Ekstensi Browser   : open-idm/browser-extension")
    print("=" * 65)
    print("Tekan Ctrl+C di terminal ini untuk menghentikan aplikasi.\n")

def open_browser():
    time.sleep(1.2)
    url = f"http://{SERVER_HOST}:{SERVER_PORT}"
    try:
        webbrowser.open(url)
    except Exception:
        pass

if __name__ == "__main__":
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace', line_buffering=True)

    print_banner()

    # Open dashboard in browser automatically
    threading.Thread(target=open_browser, daemon=True).start()

    # Run core server
    run_server(host=SERVER_HOST, port=SERVER_PORT)
