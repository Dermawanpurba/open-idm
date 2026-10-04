import re
import time
import urllib.parse
from typing import Dict, Any, List, Optional, Tuple
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

def determine_category(ext: str, mime: str = "", url: str = "") -> str:
    ext = ext.lower().strip(".")
    video_exts = {"mp4", "mkv", "webm", "avi", "mov", "flv", "ts", "m4v"}
    audio_exts = {"mp3", "m4a", "wav", "flac", "aac", "ogg", "opus"}
    doc_exts = {"pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt"}
    prog_exts = {"exe", "msi", "apk", "dmg", "iso", "bin", "deb"}
    zip_exts = {"zip", "rar", "7z", "tar", "gz", "bz2"}

    if any(domain in url.lower() for domain in ["vod3.cf.dmcdn.net", "dmcdn.net", "dailymotion.com"]):
        return "Video"

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
            ".mp4", ".mkv", ".webm", ".avi", ".mov", ".flv", ".ts", ".m4v",
            ".mp3", ".m4a", ".wav", ".flac",
            ".zip", ".rar", ".7z", ".tar", ".gz",
            ".exe", ".msi", ".iso", ".pdf"
        ]
        if any(path.endswith(ext) for ext in direct_extensions):
            return True
        # Explicit check for Dailymotion Video on Demand CDN
        if any(domain in url.lower() for domain in ["vod3.cf.dmcdn.net", "dmcdn.net"]):
            if not path.endswith(".m3u8"):
                return True
        return False

    @classmethod
    def get_direct_file_info(cls, url: str) -> Optional[Dict[str, Any]]:
        try:
            headers = {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
            }
            is_dm = any(domain in url.lower() for domain in ["vod3.cf.dmcdn.net", "dmcdn.net", "dailymotion.com"])
            if is_dm:
                headers["Referer"] = "https://www.dailymotion.com/"
                headers["Origin"] = "https://www.dailymotion.com"

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
            if not filename or filename == "download_file" or not ("." in filename):
                if is_dm:
                    filename = f"Dailymotion_Video_{int(time.time())}.mp4"
                else:
                    filename = filename or "download_file"

            filename = sanitize_filename(filename)
            ext = filename.split(".")[-1] if "." in filename else "bin"
            category = determine_category(ext, content_type, url)

            is_video = category == "Video" or is_dm

            return {
                "is_stream": False,
                "url": res.url,
                "title": filename,
                "filename": filename,
                "filesize": content_len,
                "filesize_formatted": format_bytes(content_len) if content_len > 0 else "Stream Langsung",
                "accept_ranges": accept_ranges,
                "category": "Video" if is_video else category,
                "thumbnail": "",
                "formats": [
                    {
                        "format_id": "direct",
                        "quality": "Direct Stream (DMCDN)" if is_dm else "Original File",
                        "resolution": "Original",
                        "ext": ext if ext != "bin" else "mp4",
                        "filesize": content_len,
                        "filesize_formatted": format_bytes(content_len) if content_len > 0 else "Stream Langsung",
                        "has_video": is_video,
                        "has_audio": True
                    }
                ]
            }
        except Exception as e:
            return None

    @classmethod
    def resolve_embedded_video(cls, url: str) -> Optional[Tuple[str, Optional[str]]]:
        # 1. Direct Dailymotion URL or geo player URL
        dm_url_match = re.search(r'(?:dailymotion\.com/(?:embed/)?video/|geo\.dailymotion\.com/player/[^?\"\']+\?video=)([a-zA-Z0-9]+)', url, re.I)
        if dm_url_match:
            return f"https://www.dailymotion.com/video/{dm_url_match.group(1)}", None

        # 2. Check if this is an OK.ru embed URL
        ok_url_match = re.search(r'(?:ok\.ru|odnoklassniki\.ru)/videoembed/(\d+)', url, re.I)
        if ok_url_match:
            return f"https://ok.ru/video/{ok_url_match.group(1)}", None

        # 3. Direct YouTube URL
        yt_direct_match = re.search(r'(?:youtube\.com/watch\?v=|youtu\.be/)([a-zA-Z0-9_-]{11})', url, re.I)
        if yt_direct_match:
            return f"https://www.youtube.com/watch?v={yt_direct_match.group(1)}", None

        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
            "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
            "Accept-Language": "en-US,en;q=0.9,id;q=0.8",
            "Sec-Ch-Ua": "\"Chromium\";v=\"124\", \"Google Chrome\";v=\"124\", \"Not-A.Brand\";v=\"99\"",
            "Sec-Ch-Ua-Mobile": "?0",
            "Sec-Ch-Ua-Platform": "\"Windows\"",
            "Sec-Fetch-Dest": "document",
            "Sec-Fetch-Mode": "navigate",
            "Sec-Fetch-Site": "none",
            "Sec-Fetch-User": "?1",
            "Upgrade-Insecure-Requests": "1"
        }

        html = None
        for attempt in range(2):
            try:
                res = requests.get(url, headers=headers, timeout=20 + (attempt * 5), allow_redirects=True)
                if res.status_code < 400:
                    html = res.text
                    break
            except Exception:
                time.sleep(1)

        if not html:
            return None

        # Extract page title
        title_match = re.search(r'<title>(.*?)</title>', html, re.I | re.S)
        page_title = title_match.group(1).strip() if title_match else None
        if page_title:
            page_title = re.sub(r'\s+', ' ', page_title).strip()
            page_title = re.sub(r'\s*-\s*Anichin.*$', '', page_title, flags=re.I).strip()

        # Check for Dailymotion in iframe / scripts / attributes (e.g. data-litespeed-src, geo.dailymotion.com)
        dm_match = re.search(r'(?:dailymotion\.com/(?:embed/)?video/|geo\.dailymotion\.com/player/[^?\"\'<>\s]+\?video=|dailymotion\.com/player/[^?\"\'<>\s]+\?video=)([a-zA-Z0-9]+)', html, re.I)
        if dm_match:
            return f"https://www.dailymotion.com/video/{dm_match.group(1)}", page_title

        # Check for OK.ru embed
        ok_match = re.search(r'(?:ok\.ru|odnoklassniki\.ru)/videoembed/(\d+)', html, re.I)
        if ok_match:
            return f"https://ok.ru/video/{ok_match.group(1)}", page_title

        # Check for YouTube embed
        yt_match = re.search(r'(?:youtube\.com/embed/|youtu\.be/)([a-zA-Z0-9_-]{11})', html, re.I)
        if yt_match:
            return f"https://www.youtube.com/watch?v={yt_match.group(1)}", page_title

        # Check for anichin.stream iframe
        stream_match = re.search(r'<iframe[^>]+(?:src|data-src|data-litespeed-src)=["\'](https?://[^"\']*(?:anichin\.stream|streaming)[^"\']*)["\']', html, re.I)
        if stream_match:
            sub_url = stream_match.group(1)
            try:
                sub_res = requests.get(sub_url, headers={**headers, "Referer": url}, timeout=15)
                if sub_res.status_code < 400:
                    sub_html = sub_res.text
                    sub_dm = re.search(r'(?:dailymotion\.com/(?:embed/)?video/|geo\.dailymotion\.com/player/[^?\"\'<>\s]+\?video=)([a-zA-Z0-9]+)', sub_html, re.I)
                    if sub_dm:
                        return f"https://www.dailymotion.com/video/{sub_dm.group(1)}", page_title
                    sub_ok = re.search(r'(?:ok\.ru|odnoklassniki\.ru)/videoembed/(\d+)', sub_html, re.I)
                    if sub_ok:
                        return f"https://ok.ru/video/{sub_ok.group(1)}", page_title
            except Exception:
                pass

        # Check for alternative video mirrors/servers on streaming sites (e.g. Anichin /v/2/ Dailymotion or Ok.ru mirror options)
        mirror_matches = re.findall(r'<option[^>]*value=["\']([^"\']+)["\'][^>]*>(.*?)</option>', html, re.I | re.S)
        for m_url, m_name in mirror_matches:
            if not m_url or m_url == url or not m_url.startswith("http"):
                continue
            m_text = m_name.lower()
            if any(k in m_text for k in ["dailymotion", "ok.ru", "stream", "server", "premium"]):
                try:
                    m_res = requests.get(m_url, headers={**headers, "Referer": url}, timeout=12)
                    if m_res.status_code < 400:
                        m_html = m_res.text
                        m_dm = re.search(r'(?:dailymotion\.com/(?:embed/)?video/|geo\.dailymotion\.com/player/[^?\"\'<>\s]+\?video=)([a-zA-Z0-9]+)', m_html, re.I)
                        if m_dm:
                            return f"https://www.dailymotion.com/video/{m_dm.group(1)}", page_title
                        m_ok = re.search(r'(?:ok\.ru|odnoklassniki\.ru)/videoembed/(\d+)', m_html, re.I)
                        if m_ok:
                            return f"https://ok.ru/video/{m_ok.group(1)}", page_title
                except Exception:
                    pass

        return None

    @staticmethod
    def make_filename(title: str, resolution: str = "", ext: str = "mp4") -> str:
        # Strip existing resolution tags like [1080p], [720p] to avoid duplicates
        clean_title = re.sub(r'\[?\b(2160p|1440p|1080p|720p|480p|360p|240p|144p)\b\]?', '', title, flags=re.I).strip()
        clean_title = re.sub(r'\s+', ' ', clean_title).strip(". ")
        clean_title = sanitize_filename(clean_title) or "video"
        ext = ext.lstrip(".") if ext else "mp4"
        if resolution and resolution not in ["Default", "Original", "Audio"]:
            return f"{clean_title} [{resolution}].{ext}"
        return f"{clean_title}.{ext}"

    @classmethod
    def estimate_format_size(cls, f: Dict[str, Any], duration: int = 0, audio_size: int = 0) -> int:
        sz = f.get("filesize") or f.get("filesize_approx") or 0
        if sz and sz > 0:
            if f.get("acodec") == "none" and audio_size > 0:
                sz += audio_size
            return int(sz)

        dur = duration or 0
        tbr = f.get("tbr") or 0
        if not tbr:
            vbr = f.get("vbr") or 0
            abr = f.get("abr") or 0
            if vbr:
                tbr = vbr + (abr or 128)
        if not tbr:
            fid = str(f.get("format_id", ""))
            m = re.search(r'[-_](\d{3,5})(?:$|\+)', fid)
            if m:
                tbr = float(m.group(1))

        if not tbr and f.get("height"):
            h = int(f.get("height") or 0)
            bitrate_map = {
                2160: 12000,
                1440: 6000,
                1080: 3600,
                720: 2200,
                480: 1200,
                360: 750,
                240: 400,
                144: 220
            }
            closest_h = min(bitrate_map.keys(), key=lambda k: abs(k - h))
            tbr = bitrate_map.get(closest_h, 1500)

        if tbr and dur > 0:
            return int((tbr * 1000 / 8) * dur)

        return 0

    @classmethod
    def get_video_info(cls, url: str) -> Dict[str, Any]:
        # If it's a direct file URL, try head first
        if cls.is_direct_file(url):
            direct_info = cls.get_direct_file_info(url)
            if direct_info and (direct_info["filesize"] > 0 or any(d in url.lower() for d in ["vod3.cf.dmcdn.net", "dmcdn.net"])):
                return direct_info

        # Check if page embeds a known video player (Dailymotion, ok.ru, YouTube, etc.)
        target_url = url
        resolved_title = None
        embedded = cls.resolve_embedded_video(url)
        if embedded:
            target_url, resolved_title = embedded

        # Use yt-dlp for rich extraction (YouTube, TikTok, Twitter, generic HTML5, etc.)
        ffmpeg_exe = get_ffmpeg_path()
        node_exe = get_node_path()

        ydl_opts = {
            "quiet": True,
            "no_warnings": True,
            "skip_download": True,
            "extract_flat": False,
            "no_color": True,
            "socket_timeout": 45,
            "retries": 20,
            "hls_use_mpegts": True,
        }
        if ffmpeg_exe:
            ydl_opts["ffmpeg_location"] = ffmpeg_exe
        if node_exe:
            ydl_opts["js_runtimes"] = {"node": {}}

        if any(d in target_url.lower() for d in ["vod3.cf.dmcdn.net", "dmcdn.net", "dailymotion.com"]):
            ydl_opts["http_headers"] = {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
                "Referer": "https://www.dailymotion.com/",
                "Origin": "https://www.dailymotion.com"
            }

        try:
            with yt_dlp.YoutubeDL(ydl_opts) as ydl:
                info = ydl.extract_info(target_url, download=False)
                if not info:
                    raise ValueError("Could not extract media info")

                title = sanitize_filename(resolved_title or info.get("title", "Video Download"))
                thumbnail = info.get("thumbnail", "")
                duration = int(info.get("duration") or 0)

                raw_formats = info.get("formats", [])
                qualities: List[Dict[str, Any]] = []

                # Find best audio format for merging
                best_audio = next(
                    (f for f in reversed(raw_formats) if f.get("acodec") != "none" and f.get("vcodec") == "none"),
                    None
                )
                audio_size = (best_audio.get("filesize") or best_audio.get("filesize_approx") or 0) if best_audio else 0
                if not audio_size and best_audio and duration > 0:
                    a_abr = best_audio.get("abr") or 128
                    audio_size = int((a_abr * 1000 / 8) * duration)

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

                        f_size = cls.estimate_format_size(f, duration=duration, audio_size=audio_size)

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
                    a_size = audio_size or (int((128 * 1000 / 8) * duration) if duration > 0 else 0)
                    qualities.append({
                        "format_id": f"{a_format_id}/bestaudio",
                        "quality": f"Audio Only ({a_ext.upper()})",
                        "resolution": "Audio",
                        "ext": a_ext,
                        "filesize": a_size,
                        "filesize_formatted": format_bytes(a_size) if a_size > 0 else "HQ Audio",
                        "has_video": False,
                        "has_audio": True
                    })

                # If no formats found or direct stream
                if not qualities:
                    default_size = info.get("filesize", 0) or info.get("filesize_approx", 0) or 0
                    qualities.append({
                        "format_id": "best",
                        "quality": "Best Available",
                        "resolution": "Default",
                        "ext": info.get("ext", "mp4"),
                        "filesize": default_size,
                        "filesize_formatted": format_bytes(default_size) if default_size > 0 else "Adaptive",
                        "has_video": True,
                        "has_audio": True
                    })

                best_choice = qualities[0]
                target_filename = cls.make_filename(title, best_choice["resolution"], best_choice["ext"])

                return {
                    "is_stream": True,
                    "url": target_url,
                    "title": title,
                    "filename": target_filename,
                    "filesize": best_choice["filesize"],
                    "filesize_formatted": best_choice["filesize_formatted"],
                    "category": "Video" if best_choice["has_video"] else "Music",
                    "thumbnail": thumbnail,
                    "duration": duration,
                    "duration_formatted": format_duration(duration),
                    "formats": qualities
                }
        except Exception as e:
            # Fallback: if we haven't already tried resolving embedded video, try it now
            if target_url == url:
                embedded = cls.resolve_embedded_video(url)
                if embedded and embedded[0] != url:
                    return cls.get_video_info(embedded[0])

            # Fallback to direct info attempt only if it is actually media (not generic HTML)
            direct = cls.get_direct_file_info(url)
            if direct and (direct.get("has_video") or direct.get("filesize", 0) > 0) and direct.get("category") != "Programs":
                return direct

            # If from vod3.cf.dmcdn.net or dmcdn.net, produce a robust video stream info
            if any(domain in url.lower() for domain in ["vod3.cf.dmcdn.net", "dmcdn.net"]):
                return {
                    "is_stream": True,
                    "url": url,
                    "title": "Dailymotion Video Stream",
                    "filename": f"Dailymotion_Video_{int(time.time())}.mp4",
                    "filesize": 0,
                    "filesize_formatted": "Stream Langsung",
                    "category": "Video",
                    "thumbnail": "",
                    "duration": 0,
                    "duration_formatted": "",
                    "formats": [
                        {
                            "format_id": "best",
                            "quality": "Direct Stream (vod3.cf.dmcdn.net)",
                            "resolution": "Original",
                            "ext": "mp4",
                            "filesize": 0,
                            "filesize_formatted": "Stream Langsung",
                            "has_video": True,
                            "has_audio": True
                        }
                    ]
                }

            raise RuntimeError(f"Gagal mengambil informasi video: {str(e)}")
