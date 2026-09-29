import os
import re
import sys
import time
import math
import threading
from pathlib import Path
from typing import Dict, Any, Callable, Optional, List
import requests
import yt_dlp

from .config import CHUNK_BUFFER_SIZE, DEFAULT_CHUNKS, DOWNLOADS_DIR, get_ffmpeg_path, get_node_path

class DownloadTask:
    def __init__(
        self,
        task_id: str,
        url: str,
        title: str,
        filename: str,
        category: str = "General",
        format_id: str = "best",
        filepath: Optional[str] = None,
        on_progress: Optional[Callable[[Dict[str, Any]], None]] = None,
        on_complete: Optional[Callable[[str], None]] = None,
        on_error: Optional[Callable[[str, str], None]] = None
    ):
        self.task_id = task_id
        self.url = url
        self.title = title
        self.filename = filename
        self.category = category
        self.format_id = format_id

        cat_folder = DOWNLOADS_DIR / self.category
        cat_folder.mkdir(parents=True, exist_ok=True)
        self.filepath = filepath or str(cat_folder / self.filename)

        self.on_progress = on_progress
        self.on_complete = on_complete
        self.on_error = on_error

        self.status = "pending"  # pending, downloading, paused, completed, error
        self.filesize = 0
        self.downloaded_bytes = 0
        self.speed = 0.0
        self.eta = 0
        self.progress = 0.0
        self.error_message = ""
        self.can_resume = True
        self.num_chunks = DEFAULT_CHUNKS
        self.chunks_data: List[Dict[str, Any]] = []

        self._stop_event = threading.Event()
        self._is_paused = False
        self._thread: Optional[threading.Thread] = None

    def start(self) -> None:
        self._stop_event.clear()
        self._is_paused = False
        self.status = "downloading"
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def pause(self) -> None:
        self._is_paused = True
        self._stop_event.set()
        self.status = "paused"
        for c in self.chunks_data:
            if c.get("status") != "Selesai":
                c["status"] = "Dijeda"
        self._notify_progress()

    def resume(self) -> None:
        if self.status == "paused":
            self.start()

    def cancel(self) -> None:
        self._stop_event.set()
        self.status = "error"
        self.error_message = "Dibatalkan oleh pengguna"

    def _notify_progress(self) -> None:
        if self.on_progress:
            self.on_progress({
                "id": self.task_id,
                "status": self.status,
                "downloaded_bytes": self.downloaded_bytes,
                "filesize": self.filesize,
                "speed": self.speed,
                "eta": self.eta,
                "progress": self.progress,
                "error_message": self.error_message,
                "can_resume": self.can_resume,
                "num_chunks": self.num_chunks,
                "chunks": self.chunks_data
            })

    def _run(self) -> None:
        try:
            clean_url = self.url.split("?")[0].lower()
            is_manifest = clean_url.endswith(".m3u8") or clean_url.endswith(".mpd")
            is_platform = any(domain in self.url.lower() for domain in [
                "youtube.com", "youtu.be", "tiktok.com", "instagram.com", "twitter.com", "x.com",
                "facebook.com", "fb.watch", "vimeo.com", "dailymotion.com", "twitch.tv", "bilibili.com",
                "soundcloud.com", "reddit.com"
            ])

            # Determine whether this is a direct media file (multi-chunk) or needs stream extraction (yt-dlp)
            use_multi_chunk = False

            if not is_manifest and not is_platform:
                try:
                    probe_headers = {
                        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
                    }
                    probe = requests.head(self.url, headers=probe_headers, allow_redirects=True, timeout=5)
                    if probe.status_code >= 400:
                        probe = requests.get(self.url, headers={**probe_headers, "Range": "bytes=0-0"}, stream=True, timeout=5)

                    ct = probe.headers.get("Content-Type", "").lower()
                    ar = probe.headers.get("Accept-Ranges", "").lower()
                    cl = int(probe.headers.get("Content-Length", 0))

                    # If it's an HTML page (like anichin.com.co or blog), it must be extracted via yt-dlp!
                    if "text/html" in ct:
                        use_multi_chunk = False
                    elif any(m in ct for m in ["video/", "audio/", "application/octet-stream", "application/x-", "binary/"]) or (cl > 0 and ("bytes" in ar or "content-range" in probe.headers)):
                        use_multi_chunk = True
                        self.can_resume = "bytes" in ar or "content-range" in probe.headers
                except Exception:
                    # Fallback check on file extension
                    direct_exts = [".mp4", ".mkv", ".webm", ".avi", ".mov", ".flv", ".mp3", ".zip", ".rar", ".exe", ".iso"]
                    if any(clean_url.endswith(ext) for ext in direct_exts):
                        use_multi_chunk = True

            if use_multi_chunk:
                self._download_multi_chunk()
            else:
                self._download_with_ytdlp()

            if not self._stop_event.is_set():
                self.status = "completed"
                self.progress = 100.0
                self.speed = 0.0
                self.eta = 0
                self._notify_progress()
                if self.on_complete:
                    self.on_complete(self.task_id)

        except Exception as e:
            if not self._stop_event.is_set():
                self.status = "error"
                raw_err = str(e)
                # Strip ANSI escape codes
                clean_err = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', raw_err).strip()
                if "ffmpeg is not installed" in clean_err.lower():
                    clean_err = "FFmpeg tidak ditemukan. Silakan pastikan bin/ffmpeg.exe tersedia untuk menggabungkan format video & audio."
                self.error_message = clean_err
                self._notify_progress()
                if self.on_error:
                    self.on_error(self.task_id, clean_err)

    def _download_with_ytdlp(self) -> None:
        last_time = time.time()
        last_bytes = 0

        def ytdl_hook(d: Dict[str, Any]) -> None:
            nonlocal last_time, last_bytes
            if self._stop_event.is_set():
                raise InterruptedError("Download dihentikan")

            status = d.get("status")
            if status == "downloading":
                total = d.get("total_bytes") or d.get("total_bytes_estimate") or 0
                downloaded = d.get("downloaded_bytes") or 0
                current_speed = d.get("speed") or 0.0
                current_eta = d.get("eta") or 0

                now = time.time()
                # Update progress every 300ms
                if now - last_time >= 0.3:
                    self.filesize = total
                    self.downloaded_bytes = downloaded
                    self.speed = current_speed
                    self.eta = current_eta
                    if total > 0:
                        self.progress = round((downloaded / total) * 100.0, 1)
                    self._notify_progress()
                    last_time = now

            elif status == "finished":
                self.downloaded_bytes = self.filesize
                self.progress = 100.0
                self._notify_progress()

        ffmpeg_exe = get_ffmpeg_path()
        node_exe = get_node_path()

        # Smart format selection for yt-dlp:
        # Never pass plain "best" to yt-dlp on YouTube/modern streams because modern streams require video+audio merging!
        if not self.format_id or self.format_id in ["best", "direct", "original"]:
            chosen_format = "bestvideo*+bestaudio/best"
        elif "/" in self.format_id or "+" in self.format_id:
            chosen_format = f"{self.format_id}/bestvideo*+bestaudio/best"
        else:
            chosen_format = f"{self.format_id}+bestaudio/bestvideo*+bestaudio/best"

        if not ffmpeg_exe and "+" in chosen_format:
            chosen_format = "best[ext=mp4]/best"

        out_template = str(Path(self.filepath).with_suffix("")) + ".%(ext)s"
        ydl_opts = {
            "format": chosen_format,
            "outtmpl": out_template,
            "progress_hooks": [ytdl_hook],
            "quiet": True,
            "no_warnings": True,
            "no_color": True,
            "nocheckcertificate": True,
            "concurrent_fragment_downloads": 8,
            "windowsfilenames": True,
            "retries": 10,
            "fragment_retries": 10,
        }
        if ffmpeg_exe:
            ydl_opts["ffmpeg_location"] = ffmpeg_exe
        if node_exe:
            ydl_opts["js_runtimes"] = {"node": {}}

        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            info = ydl.extract_info(self.url, download=True)
            if info:
                # Update actual resulting file path
                actual_filename = ydl.prepare_filename(info)
                if not os.path.exists(actual_filename):
                    req_dl = info.get("requested_downloads")
                    if req_dl and isinstance(req_dl, list) and req_dl[0].get("filepath"):
                        actual_filename = req_dl[0]["filepath"]
                    else:
                        base_stem = Path(actual_filename).stem
                        parent_dir = Path(actual_filename).parent
                        if parent_dir.exists():
                            for candidate in parent_dir.iterdir():
                                if candidate.stem == base_stem and candidate.suffix not in ['.part', '.ytdl', '.temp']:
                                    actual_filename = str(candidate)
                                    break

                self.filepath = actual_filename
                self.filename = os.path.basename(actual_filename)
                if os.path.exists(self.filepath):
                    self.filesize = os.path.getsize(self.filepath)

    def _download_multi_chunk(self) -> None:
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
        }
        res = requests.head(self.url, headers=headers, allow_redirects=True, timeout=10)
        self.filesize = int(res.headers.get("Content-Length", 0))
        accept_ranges = "bytes" in res.headers.get("Accept-Ranges", "").lower()

        # If server does not support Range requests or filesize is small (< 2MB), download single stream
        if not accept_ranges or self.filesize < 2 * 1024 * 1024:
            self._download_single_stream(headers)
            return

        # Multi-chunk segmented downloading (IDM-style 8 chunks)
        num_chunks = DEFAULT_CHUNKS
        self.num_chunks = num_chunks
        self.can_resume = accept_ranges
        chunk_size = math.ceil(self.filesize / num_chunks)
        part_files: List[str] = []
        chunk_progress: List[int] = [0] * num_chunks
        threads: List[threading.Thread] = []

        chunk_dir = Path(self.filepath + ".chunks")
        chunk_dir.mkdir(parents=True, exist_ok=True)

        self.chunks_data = []
        for i in range(num_chunks):
            part_path = str(chunk_dir / f"part_{i}.tmp")
            part_files.append(part_path)
            start_byte = i * chunk_size
            end_byte = min(start_byte + chunk_size - 1, self.filesize - 1)

            # Check if part already partially exists for resume
            existing_bytes = os.path.getsize(part_path) if os.path.exists(part_path) else 0
            chunk_progress[i] = existing_bytes
            part_total = max(1, end_byte - start_byte + 1)

            self.chunks_data.append({
                "num": i + 1,
                "start": start_byte,
                "end": end_byte,
                "total": part_total,
                "downloaded": existing_bytes,
                "percent": round((existing_bytes / part_total) * 100, 1),
                "status": "Menerima data..." if existing_bytes < part_total else "Selesai"
            })

            t = threading.Thread(
                target=self._worker_download_chunk,
                args=(i, start_byte + existing_bytes, end_byte, part_path, chunk_progress, headers),
                daemon=True
            )
            threads.append(t)
            t.start()

        # Progress monitor loop
        last_downloaded = sum(chunk_progress)
        last_time = time.time()

        while any(t.is_alive() for t in threads):
            if self._stop_event.is_set():
                for c in self.chunks_data:
                    if c.get("status") != "Selesai":
                        c["status"] = "Dijeda"
                self._notify_progress()
                return

            time.sleep(0.3)
            current_downloaded = sum(chunk_progress)
            now = time.time()
            elapsed = now - last_time
            if elapsed > 0:
                self.speed = (current_downloaded - last_downloaded) / elapsed
                last_downloaded = current_downloaded
                last_time = now

            self.downloaded_bytes = current_downloaded
            if self.filesize > 0:
                self.progress = round((current_downloaded / self.filesize) * 100.0, 1)
                remaining_bytes = self.filesize - current_downloaded
                self.eta = int(remaining_bytes / self.speed) if self.speed > 0 else 0

            # Update live chunks info
            for i in range(num_chunks):
                cur_dl = chunk_progress[i]
                tot_p = self.chunks_data[i]["total"]
                self.chunks_data[i]["downloaded"] = cur_dl
                self.chunks_data[i]["percent"] = round((cur_dl / tot_p) * 100.0, 1) if tot_p > 0 else 0
                if cur_dl >= tot_p:
                    self.chunks_data[i]["status"] = "Selesai"
                elif self._is_paused:
                    self.chunks_data[i]["status"] = "Dijeda"
                else:
                    self.chunks_data[i]["status"] = "Menerima data..."

            self._notify_progress()

        for t in threads:
            t.join()

        if self._stop_event.is_set():
            return

        # Merge all chunks into final file
        with open(self.filepath, "wb") as outfile:
            for part in part_files:
                if os.path.exists(part):
                    with open(part, "rb") as infile:
                        while chunk := infile.read(CHUNK_BUFFER_SIZE * 4):
                            outfile.write(chunk)
                    try:
                        os.remove(part)
                    except OSError:
                        pass

        try:
            chunk_dir.rmdir()
        except OSError:
            pass

    def _worker_download_chunk(
        self,
        index: int,
        start_byte: int,
        end_byte: int,
        part_path: str,
        chunk_progress: List[int],
        headers: Dict[str, str]
    ) -> None:
        if start_byte > end_byte:
            return

        req_headers = {**headers, "Range": f"bytes={start_byte}-{end_byte}"}
        try:
            with requests.get(self.url, headers=req_headers, stream=True, timeout=15) as r:
                r.raise_for_status()
                mode = "ab" if os.path.exists(part_path) else "wb"
                with open(part_path, mode) as f:
                    for chunk in r.iter_content(chunk_size=CHUNK_BUFFER_SIZE):
                        if self._stop_event.is_set():
                            return
                        if chunk:
                            f.write(chunk)
                            chunk_progress[index] += len(chunk)
        except Exception as e:
            if not self._stop_event.is_set():
                raise e

    def _download_single_stream(self, headers: Dict[str, str]) -> None:
        last_time = time.time()
        last_bytes = 0

        with requests.get(self.url, headers=headers, stream=True, timeout=15) as r:
            r.raise_for_status()
            with open(self.filepath, "wb") as f:
                for chunk in r.iter_content(chunk_size=CHUNK_BUFFER_SIZE):
                    if self._stop_event.is_set():
                        return
                    if chunk:
                        f.write(chunk)
                        self.downloaded_bytes += len(chunk)

                        now = time.time()
                        if now - last_time >= 0.3:
                            elapsed = now - last_time
                            self.speed = (self.downloaded_bytes - last_bytes) / elapsed if elapsed > 0 else 0
                            last_bytes = self.downloaded_bytes
                            last_time = now

                            if self.filesize > 0:
                                self.progress = round((self.downloaded_bytes / self.filesize) * 100.0, 1)
                                remaining = self.filesize - self.downloaded_bytes
                                self.eta = int(remaining / self.speed) if self.speed > 0 else 0
                            self._notify_progress()
