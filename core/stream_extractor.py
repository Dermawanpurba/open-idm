import re
import urllib.parse
from typing import Dict, Any, List, Optional
import requests
import yt_dlp

# Patch yt-dlp OdnoklassnikiIE metadata issue (used by anime streaming sites like Anichin/ok.ru)
try:
    from yt_dlp.extractor.odnoklassniki import OdnoklassnikiIE
    _orig_ok_parse = OdnoklassnikiIE._parse_json
    def _safe_ok_parse(self, json_string, *args, **kwargs):
        if isinstance(json_string, (dict, list)):
            return json_string
        return _orig_ok_parse(self, json_string, *args, **kwargs)
    OdnoklassnikiIE._parse_json = _safe_ok_parse
except Exception:
    pass

from .config import get_ffmpeg_path, get_node_path

def format_bytes(size: Optional[int]) -> str:
    if not size or size <= 0:
        return "Unknown size"
    for unit in ["B", "KB", "MB", "GB", "TB"]:
        if size < 1024.0:
            return f"{size:.1f} {unit}"
        size /= 1024.0
    return f"{size:.1f} PB"

def format_duration(seconds: Optional[int]) -> str:
    if not seconds:
        return ""
    m, s = divmod(int(seconds), 60)
    h, m = divmod(m, 60)
    if h > 0:
        return f"{h:02d}:{m:02d}:{s:02d}"
    return f"{m:02d}:{s:02d}"

def sanitize_filename(name: str) -> str:
    # Remove characters invalid in Windows filenames: \ / : * ? " < > |
    cleaned = re.sub(r'[\\/*?:"<>|]', "", name)
    cleaned = re.sub(r'\s+', " ", cleaned).strip()
    cleaned = cleaned[:180].rstrip(". ")
    return cleaned or "download"

def determine_category(ext: str, mime: str = "") -> str:
    ext = ext.lower().strip(".")
    video_exts = {"mp4", "mkv", "webm", "avi", "mov", "flv", "ts", "m4v"}
    audio_exts = {"mp3", "m4a", "wav", "flac", "aac", "ogg", "opus"}
    doc_exts = {"pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt"}
    prog_exts = {"exe", "msi", "apk", "dmg", "iso", "bin", "deb"}
    zip_exts = {"zip", "rar", "7z", "tar", "gz", "bz2"}

    if ext in video_exts or "video" in mime:
        return "Video"
    if ext in audio_exts or "audio" in mime:
        return "Music"
    if ext in doc_exts or "document" in mime or "pdf" in mime:
        return "Documents"
    if ext in prog_exts or "application/x-msdownload" in mime:
        return "Programs"
    if ext in zip_exts or "compressed" in mime or "zip" in mime:
        return "Compressed"
    return "General"

class StreamExtractor:
    @staticmethod
    def is_direct_file(url: str) -> bool:
        # Check standard file extensions
        parsed = urllib.parse.urlparse(url)
        path = parsed.path.lower()
        direct_extensions = [
            ".mp4", ".mkv", ".webm", ".avi", ".mov", ".flv",
            ".mp3", ".m4a", ".wav", ".flac",
            ".zip", ".rar", ".7z", ".tar", ".gz",
            ".exe", ".msi", ".iso", ".pdf"
        ]
        return any(path.endswith(ext) for ext in direct_extensions)

    @classmethod
    def get_direct_file_info(cls, url: str) -> Optional[Dict[str, Any]]:
        try:
            headers = {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
            }
            res = requests.head(url, headers=headers, allow_redirects=True, timeout=8)
            if res.status_code >= 400:
                # Try GET with range 0-0 in case server disallows HEAD
                res = requests.get(url, headers={**headers, "Range": "bytes=0-0"}, stream=True, timeout=8)

            content_len = int(res.headers.get("Content-Length", 0))
            if "content-range" in res.headers:
                range_match = re.search(r"/(\d+)", res.headers["content-range"])
                if range_match:
                    content_len = int(range_match.group(1))

            accept_ranges = "bytes" in res.headers.get("Accept-Ranges", "").lower() or "content-range" in res.headers
            content_type = res.headers.get("Content-Type", "").split(";")[0].strip()

            # Filename extraction from header or URL
            filename = ""
            cd = res.headers.get("Content-Disposition", "")
            if cd and "filename=" in cd:
                fname_match = re.search(r'filename\*?=(?:UTF-8\'\')?"?([^";]+)"?', cd)
                if fname_match:
                    filename = urllib.parse.unquote(fname_match.group(1))
            if not filename:
                parsed = urllib.parse.urlparse(res.url)
                filename = urllib.parse.unquote(parsed.path.split("/")[-1])
            if not filename:
                filename = "download_file"

            filename = sanitize_filename(filename)
            ext = filename.split(".")[-1] if "." in filename else "bin"
            category = determine_category(ext, content_type)

            return {
                "is_stream": False,
                "url": res.url,
                "title": filename,
                "filename": filename,
                "filesize": content_len,
                "filesize_formatted": format_bytes(content_len),
                "accept_ranges": accept_ranges,
                "category": category,
                "thumbnail": "",
                "formats": [
                    {
                        "format_id": "direct",
                        "quality": "Original File",
                        "resolution": "Original",
                        "ext": ext,
                        "filesize": content_len,
                        "filesize_formatted": format_bytes(content_len),
                        "has_video": category == "Video",
                        "has_audio": category in ["Video", "Music"]
                    }
                ]
            }
        except Exception as e:
            return None

    @classmethod
    def get_video_info(cls, url: str) -> Dict[str, Any]:
        # If it's a direct file URL, try head first
        if cls.is_direct_file(url):
            direct_info = cls.get_direct_file_info(url)
            if direct_info and direct_info["filesize"] > 0:
                return direct_info

        # Use yt-dlp for rich extraction (YouTube, TikTok, Twitter, generic HTML5, etc.)
        ffmpeg_exe = get_ffmpeg_path()
        node_exe = get_node_path()

        ydl_opts = {
            "quiet": True,
            "no_warnings": True,
            "skip_download": True,
            "extract_flat": False,
            "no_color": True,
        }
        if ffmpeg_exe:
            ydl_opts["ffmpeg_location"] = ffmpeg_exe
        if node_exe:
            ydl_opts["js_runtimes"] = {"node": {}}

        try:
            with yt_dlp.YoutubeDL(ydl_opts) as ydl:
                info = ydl.extract_info(url, download=False)
                if not info:
                    raise ValueError("Could not extract media info")

                title = sanitize_filename(info.get("title", "Video Download"))
                thumbnail = info.get("thumbnail", "")
                duration = info.get("duration", 0)

                raw_formats = info.get("formats", [])
                qualities: List[Dict[str, Any]] = []

                # Find best audio format for merging
                best_audio = next(
                    (f for f in reversed(raw_formats) if f.get("acodec") != "none" and f.get("vcodec") == "none"),
                    None
                )
                audio_size = (best_audio.get("filesize") or best_audio.get("filesize_approx") or 0) if best_audio else 0

                seen_resolutions = set()
                # Sort formats from highest resolution to lowest
                for f in reversed(raw_formats):
                    height = f.get("height")
                    vcodec = f.get("vcodec")
                    format_id = f.get("format_id")
                    ext = f.get("ext", "mp4")

                    # We want video streams with defined heights (1080, 720, 480, 360, etc.)
                    if height and height >= 144 and vcodec != "none":
                        res_label = f"{height}p"
                        if res_label in seen_resolutions:
                            continue
                        seen_resolutions.add(res_label)

                        f_size = f.get("filesize") or f.get("filesize_approx") or 0
                        if f.get("acodec") == "none" and audio_size:
                            f_size += audio_size

                        qualities.append({
                            "format_id": f"{format_id}+bestaudio/best" if f.get("acodec") == "none" else format_id,
                            "quality": f"{res_label} ({ext.upper()})",
                            "resolution": res_label,
                            "ext": "mp4" if ext == "webm" else ext,
                            "filesize": f_size,
                            "filesize_formatted": format_bytes(f_size) if f_size > 0 else "Adaptive",
                            "has_video": True,
                            "has_audio": True
                        })

                # Add Audio-Only option
                if best_audio:
                    a_format_id = best_audio.get("format_id", "bestaudio")
                    a_ext = best_audio.get("ext", "m4a")
                    qualities.append({
                        "format_id": f"{a_format_id}/bestaudio",
                        "quality": f"Audio Only ({a_ext.upper()})",
                        "resolution": "Audio",
                        "ext": a_ext,
                        "filesize": audio_size,
                        "filesize_formatted": format_bytes(audio_size) if audio_size > 0 else "HQ Audio",
                        "has_video": False,
                        "has_audio": True
                    })

                # If no formats found or direct stream
                if not qualities:
                    qualities.append({
                        "format_id": "best",
                        "quality": "Best Available",
                        "resolution": "Default",
                        "ext": info.get("ext", "mp4"),
                        "filesize": info.get("filesize", 0) or 0,
                        "filesize_formatted": format_bytes(info.get("filesize")),
                        "has_video": True,
                        "has_audio": True
                    })

                best_choice = qualities[0]
                return {
                    "is_stream": True,
                    "url": url,
                    "title": title,
                    "filename": f"{title}.{best_choice['ext']}",
                    "filesize": best_choice["filesize"],
                    "filesize_formatted": best_choice["filesize_formatted"],
                    "category": "Video" if best_choice["has_video"] else "Music",
                    "thumbnail": thumbnail,
                    "duration": duration,
                    "duration_formatted": format_duration(duration),
                    "formats": qualities
                }
        except Exception as e:
            # Fallback to direct info attempt
            direct = cls.get_direct_file_info(url)
            if direct:
                return direct
            raise RuntimeError(f"Gagal mengambil informasi video: {str(e)}")
