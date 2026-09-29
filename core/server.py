import os
import json
import urllib.parse
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Dict, Any

from .config import BASE_DIR, SERVER_HOST, SERVER_PORT, get_ffmpeg_path
from .task_manager import TaskManager

task_manager = TaskManager()
UI_DIR = BASE_DIR / "ui"

class OpenIDMRequestHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(UI_DIR), **kwargs)

    def _set_cors_headers(self, status: int = 200, content_type: str = "application/json"):
        self.send_response(status)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, Authorization")
        if content_type:
            self.send_header("Content-Type", content_type)

    def do_OPTIONS(self):
        self._set_cors_headers(204, content_type="")
        self.end_headers()

    def _send_json(self, data: Any, status: int = 200):
        self._set_cors_headers(status, "application/json")
        payload = json.dumps(data).encode("utf-8")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def _read_json_body(self) -> Dict[str, Any]:
        content_len = int(self.headers.get("Content-Length", 0))
        if content_len == 0:
            return {}
        body = self.rfile.read(content_len).decode("utf-8")
        try:
            return json.loads(body)
        except Exception:
            return {}

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path
        query = urllib.parse.parse_qs(parsed.query)

        if path == "/api/status":
            downloads = task_manager.get_all_tasks()
            active = len([d for d in downloads if d["status"] == "downloading"])
            ffmpeg_path = get_ffmpeg_path()
            self._send_json({
                "status": "online",
                "app": "OpenIDM Core",
                "version": "1.0.0",
                "active_downloads": active,
                "total_downloads": len(downloads),
                "ffmpeg_available": bool(ffmpeg_path),
                "ffmpeg_path": ffmpeg_path or ""
            })
            return

        if path == "/api/downloads":
            category = query.get("category", [None])[0]
            status = query.get("status", [None])[0]
            downloads = task_manager.get_all_tasks(category=category, status=status)
            self._send_json({"success": True, "downloads": downloads})
            return

        # Serve static UI files
        if path == "/" or not path.startswith("/api/"):
            super().do_GET()
            return

        self._send_json({"error": "Endpoint not found"}, status=404)

    def do_POST(self):
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path
        body = self._read_json_body()

        if path == "/api/info":
            url = body.get("url", "").strip()
            if not url:
                self._send_json({"error": "URL diperlukan"}, status=400)
                return
            try:
                info = task_manager.get_video_info(url)
                self._send_json({"success": True, "info": info})
            except Exception as e:
                self._send_json({"error": str(e)}, status=500)
            return

        if path == "/api/download":
            url = body.get("url", "").strip()
            if not url:
                self._send_json({"error": "URL diperlukan"}, status=400)
                return

            task = task_manager.create_download(
                url=url,
                title=body.get("title"),
                filename=body.get("filename"),
                category=body.get("category", "General"),
                format_id=body.get("format_id", "best"),
                thumbnail=body.get("thumbnail", ""),
                filesize=body.get("filesize", 0)
            )
            self._send_json({"success": True, "task": task})
            return

        if path == "/api/control":
            task_id = body.get("id")
            action = body.get("action")
            delete_file = body.get("delete_file", False)

            if not task_id or not action:
                self._send_json({"error": "ID dan action diperlukan"}, status=400)
                return

            success = False
            if action == "pause":
                success = task_manager.pause_task(task_id)
            elif action == "resume":
                success = task_manager.resume_task(task_id)
            elif action == "cancel":
                success = task_manager.cancel_task(task_id)
            elif action == "delete":
                success = task_manager.delete_task(task_id, delete_file=delete_file)

            self._send_json({"success": success})
            return

        if path == "/api/open-file":
            task_id = body.get("id")
            success = task_manager.open_file(task_id)
            self._send_json({"success": success})
            return

        if path == "/api/open-folder":
            task_id = body.get("id")
            success = task_manager.open_folder(task_id)
            self._send_json({"success": success})
            return

        self._send_json({"error": "Endpoint not found"}, status=404)

def run_server(host: str = SERVER_HOST, port: int = SERVER_PORT):
    server = ThreadingHTTPServer((host, port), OpenIDMRequestHandler)
    print(f"[OpenIDM Core] Server aktif di http://{host}:{port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[OpenIDM Core] Server dihentikan.")
        server.server_close()
