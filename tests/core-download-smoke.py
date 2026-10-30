"""Exercise the actual API/downloader with isolated SQLite and download folders."""
import json
import gc
import sys
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


def main():
    # Configure isolation before importing modules that bind these paths.
    with tempfile.TemporaryDirectory(prefix="openidm-core-test-") as directory:
        from core import config
        config.DB_PATH = Path(directory) / "downloads.db"
        config.DOWNLOADS_DIR = Path(directory) / "downloads"
        from core.server import OpenIDMRequestHandler, task_manager

        content = bytes(range(256)) * 4096

        class Fixture(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_HEAD(self):
                self.send_response(200)
                self.send_header("Content-Type", "video/mp4")
                self.send_header("Content-Length", str(len(content)))
                self.send_header("Accept-Ranges", "bytes")
                self.end_headers()

            def do_GET(self):
                start, end = 0, len(content) - 1
                partial = self.headers.get("Range")
                if partial:
                    bounds = partial.removeprefix("bytes=").split("-")
                    start = int(bounds[0])
                    end = int(bounds[1]) if bounds[1] else end
                self.send_response(206 if partial else 200)
                self.send_header("Content-Type", "video/mp4")
                self.send_header("Content-Length", str(end - start + 1))
                self.send_header("Accept-Ranges", "bytes")
                if partial:
                    self.send_header("Content-Range", f"bytes {start}-{end}/{len(content)}")
                self.end_headers()
                self.wfile.write(content[start:end + 1])

        fixture = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
        server = ThreadingHTTPServer(("127.0.0.1", 0), OpenIDMRequestHandler)
        for instance in (fixture, server):
            threading.Thread(target=instance.serve_forever, daemon=True).start()
        api = f"http://127.0.0.1:{server.server_port}"

        def request(endpoint, body=None):
            req = urllib.request.Request(api + endpoint,
                data=json.dumps(body).encode() if body is not None else None,
                headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(req, timeout=15) as response:
                return json.load(response)

        try:
            assert request("/api/status")["app"] == "OpenIDM Core"
            media_url = f"http://127.0.0.1:{fixture.server_port}/fixture.mp4"
            info = request("/api/info", {"url": media_url})
            assert info["success"] and info["info"]["formats"]
            result = request("/api/download", {"url": media_url, "title": "Extension API smoke", "filename": "fixture.mp4", "category": "Video", "format_id": "direct", "filesize": len(content), "headers": {"Referer": media_url}})
            assert result["success"]
            task_id = result["task"]["id"]
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                task = next(item for item in request("/api/downloads")["downloads"] if item["id"] == task_id)
                if task["status"] == "completed":
                    break
                assert task["status"] != "error", task.get("error_message")
                time.sleep(0.1)
            assert task["status"] == "completed", task
            assert Path(task["filepath"]).read_bytes() == content
            assert Path(task["filepath"]).is_relative_to(config.DOWNLOADS_DIR)
            print("PASS: real Python status/info/download API; downloaded 1 MiB matches source bytes")
            print("PASS: temporary database and download directory; existing user data untouched")
        finally:
            for task in list(task_manager.active_tasks.values()):
                task.cancel()
                if task._thread:
                    task._thread.join(timeout=5)
            for instance in (fixture, server):
                instance.shutdown()
                instance.server_close()
            # sqlite connection context managers commit; collection closes handles.
            gc.collect()


if __name__ == "__main__":
    main()
