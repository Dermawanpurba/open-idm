import os
import re
import sys
import time
import uuid
import subprocess
from typing import Dict, Any, List, Optional
from pathlib import Path

from .database import DatabaseManager
from .segmented_downloader import DownloadTask
from .stream_extractor import StreamExtractor, sanitize_filename

class TaskManager:
    def __init__(self):
        self.db = DatabaseManager()
        self.active_tasks: Dict[str, DownloadTask] = {}

    def get_video_info(self, url: str) -> Dict[str, Any]:
        return StreamExtractor.get_video_info(url)

    def create_download(
        self,
        url: str,
        title: Optional[str] = None,
        filename: Optional[str] = None,
        category: str = "General",
        format_id: str = "best",
        thumbnail: str = "",
        filesize: int = 0,
        headers: Optional[Dict[str, str]] = None
    ) -> Dict[str, Any]:
        task_id = str(uuid.uuid4())[:8]

        # Resolve URL if it's an embedding page (e.g. anichin, dailymotion embeds, etc.)
        clean_url = url.split("?")[0].lower()
        direct_exts = (".mp4", ".mkv", ".webm", ".m3u8", ".mpd", ".flv", ".ts", ".mp3", ".zip", ".rar", ".exe", ".iso")
        if not clean_url.endswith(direct_exts) and not any(d in url.lower() for d in ["vod3.cf.dmcdn.net", "dmcdn.net"]):
            try:
                embedded = StreamExtractor.resolve_embedded_video(url)
                if embedded:
                    url = embedded[0]
                    if (not title or title == "Download") and embedded[1]:
                        title = embedded[1]
            except Exception:
                pass

        # Extract info to get accurate format details and filesizes
        info = None
        try:
            info = StreamExtractor.get_video_info(url)
        except Exception:
            pass

        if info:
            url = info.get("url", url)
            title = title or info.get("title", "Download")
            category = category if category != "General" else info.get("category", "General")
            thumbnail = thumbnail or info.get("thumbnail", "")

            # Look up matching format
            formats = info.get("formats", [])
            matched_fmt = None
            if format_id and format_id != "best":
                matched_fmt = next((f for f in formats if f.get("format_id") == format_id), None)
                if not matched_fmt:
                    fid_base = format_id.split("+")[0].split("/")[0]
                    matched_fmt = next((f for f in formats if f.get("format_id", "").startswith(fid_base)), None)

            if matched_fmt:
                # Format matched! Update filesize to specific format size
                fmt_size = matched_fmt.get("filesize", 0)
                if fmt_size > 0:
                    filesize = fmt_size
                res = matched_fmt.get("resolution", "")
                ext = matched_fmt.get("ext", "mp4")
                if not filename or filename in ["video.mp4", "file.bin"] or not re.search(r'\[?\b\d{3,4}p\b\]?', filename):
                    filename = StreamExtractor.make_filename(title, res, ext)
            else:
                if not filesize:
                    filesize = info.get("filesize", 0)
                if not filename:
                    filename = info.get("filename", "video.mp4")
        else:
            title = title or "Direct Download"
            filename = filename or "file.bin"

        title = sanitize_filename(title)
        filename = sanitize_filename(filename)

        # Create runner
        task = DownloadTask(
            task_id=task_id,
            url=url,
            title=title,
            filename=filename,
            category=category,
            format_id=format_id,
            filesize=filesize,
            headers=headers or {},
            on_progress=self._on_task_progress,
            on_complete=self._on_task_complete,
            on_error=self._on_task_error
        )

        task_data = {
            "id": task_id,
            "url": url,
            "title": title,
            "filename": filename,
            "filepath": task.filepath,
            "filesize": filesize,
            "downloaded_bytes": 0,
            "progress": 0.0,
            "speed": 0.0,
            "eta": 0,
            "status": "downloading",
            "category": category,
            "thumbnail": thumbnail,
            "format_id": format_id,
            "headers": headers or {},
            "error_message": "",
            "created_at": time.time(),
            "completed_at": None
        }

        self.db.add_download(task_data)
        self.active_tasks[task_id] = task
        task.start()

        return task_data

    def _on_task_progress(self, data: Dict[str, Any]) -> None:
        self.db.update_progress(
            download_id=data["id"],
            downloaded_bytes=data["downloaded_bytes"],
            filesize=data["filesize"],
            speed=data["speed"],
            eta=data["eta"],
            progress=data["progress"]
        )

    def _on_task_complete(self, task_id: str) -> None:
        task = self.active_tasks.get(task_id)
        if task:
            self.db.update_file_info(task_id, task.filepath, task.filename, task.filesize)
        self.db.update_status(task_id, "completed", completed_at=time.time())
        if task_id in self.active_tasks:
            del self.active_tasks[task_id]

    def _on_task_error(self, task_id: str, error_msg: str) -> None:
        self.db.update_status(task_id, "error", error_message=error_msg)
        if task_id in self.active_tasks:
            del self.active_tasks[task_id]

    def pause_task(self, task_id: str) -> bool:
        if task_id in self.active_tasks:
            self.active_tasks[task_id].pause()
            self.db.update_status(task_id, "paused")
            return True
        return False

    def resume_task(self, task_id: str) -> bool:
        if task_id in self.active_tasks:
            self.active_tasks[task_id].resume()
            self.db.update_status(task_id, "downloading", error_message="")
            return True

        # If not in memory, re-instantiate from database
        item = self.db.get_by_id(task_id)
        if item and item["status"] in ["paused", "error", "pending"]:
            url = item["url"]
            headers_raw = item.get("headers", "")
            headers_dict = {}
            if headers_raw:
                try:
                    import json
                    headers_dict = json.loads(headers_raw) if isinstance(headers_raw, str) else headers_raw
                except Exception:
                    pass

            clean_url = url.split("?")[0].lower()
            direct_exts = (".mp4", ".mkv", ".webm", ".m3u8", ".mpd", ".flv", ".ts", ".mp3", ".zip", ".rar", ".exe", ".iso")
            if not clean_url.endswith(direct_exts) and not any(d in url.lower() for d in ["vod3.cf.dmcdn.net", "dmcdn.net"]):
                try:
                    embedded = StreamExtractor.resolve_embedded_video(url)
                    if embedded:
                        url = embedded[0]
                        self.db.update_url(task_id, url)
                except Exception:
                    pass

            task = DownloadTask(
                task_id=task_id,
                url=url,
                title=item["title"],
                filename=item["filename"],
                category=item["category"],
                format_id=item.get("format_id") or "best",
                filesize=item.get("filesize", 0),
                filepath=item["filepath"],
                headers=headers_dict,
                on_progress=self._on_task_progress,
                on_complete=self._on_task_complete,
                on_error=self._on_task_error
            )
            self.active_tasks[task_id] = task
            self.db.update_status(task_id, "downloading", error_message="")
            task.start()
            return True
        return False

    def cancel_task(self, task_id: str) -> bool:
        if task_id in self.active_tasks:
            self.active_tasks[task_id].cancel()
            del self.active_tasks[task_id]
        self.db.update_status(task_id, "error", error_message="Dibatalkan")
        return True

    def delete_task(self, task_id: str, delete_file: bool = False) -> bool:
        if task_id in self.active_tasks:
            self.active_tasks[task_id].cancel()
            del self.active_tasks[task_id]

        item = self.db.get_by_id(task_id)
        if item and delete_file and item.get("filepath"):
            try:
                if os.path.exists(item["filepath"]):
                    os.remove(item["filepath"])
            except OSError:
                pass

        return self.db.delete_by_id(task_id)

    def get_all_tasks(self, category: Optional[str] = None, status: Optional[str] = None) -> List[Dict[str, Any]]:
        tasks = self.db.get_all(category=category, status=status)
        # Augment with live in-memory stats
        for t in tasks:
            t_id = t["id"]
            if t_id in self.active_tasks:
                live = self.active_tasks[t_id]
                t["downloaded_bytes"] = live.downloaded_bytes
                t["filesize"] = live.filesize
                t["progress"] = live.progress
                t["speed"] = live.speed
                t["eta"] = live.eta
                t["status"] = live.status
                t["can_resume"] = getattr(live, "can_resume", True)
                t["num_chunks"] = getattr(live, "num_chunks", 8)
                t["chunks"] = getattr(live, "chunks_data", [])
        return tasks

    def open_file(self, task_id: str) -> bool:
        item = self.db.get_by_id(task_id)
        if item and item.get("filepath") and os.path.exists(item["filepath"]):
            try:
                os.startfile(item["filepath"])
                return True
            except Exception:
                return False
        return False

    def open_folder(self, task_id: str) -> bool:
        item = self.db.get_by_id(task_id)
        if item and item.get("filepath"):
            folder = os.path.dirname(item["filepath"])
            if os.path.exists(folder):
                try:
                    subprocess.Popen(f'explorer /select,"{item["filepath"]}"')
                    return True
                except Exception:
                    os.startfile(folder)
                    return True
        return False
