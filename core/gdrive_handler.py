import re
import time
import json
import urllib.parse
from html import unescape
from html.parser import HTMLParser
from typing import Dict, Any, List, Optional, Tuple
import requests

from .stream_extractor import format_bytes, sanitize_filename, determine_category

class GoogleDriveFolderError(RuntimeError):
    pass


class _DownloadFormParser(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.action = None
        self.inputs = {}
        self.in_download_form = False

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "form":
            self.in_download_form = attrs.get("id") == "download-form"
            if self.in_download_form:
                self.action = attrs.get("action")
        elif tag == "input" and self.in_download_form and attrs.get("name"):
            self.inputs[attrs["name"]] = attrs.get("value", "")

    def handle_endtag(self, tag):
        if tag == "form":
            self.in_download_form = False


class GoogleDriveHandler:
    FOLDER_REGEX = re.compile(
        r'drive\.google\.com/(?:drive/(?:u/\d+/)?folders/|open\?(?:.*&)?id=|folderview\?(?:.*&)?id=)([a-zA-Z0-9_-]+)',
        re.I
    )
    FILE_REGEX = re.compile(
        r'(?:drive\.google\.com/(?:file/d/|(?:u/\d+/)?file/d/|uc\?(?:.*&)?id=|open\?(?:.*&)?id=)|drive\.usercontent\.google\.com/download\?(?:.*&)?id=|docs\.google\.com/(?:document|spreadsheets|presentation)/d/)([a-zA-Z0-9_-]+)',
        re.I
    )

    HEADERS = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        "Accept-Language": "en-US,en;q=0.9,id;q=0.8"
    }

    @classmethod
    def is_gdrive_url(cls, url: str) -> bool:
        if not url:
            return False
        clean = url.lower()
        return "drive.google.com" in clean or "drive.usercontent.google.com" in clean or "docs.google.com" in clean

    @classmethod
    def get_info(cls, url: str) -> Dict[str, Any]:
        # Check folder first
        folder_match = cls.FOLDER_REGEX.search(url)
        # Note: if URL has /folders/, it is definitely a folder
        if "/folders/" in url.lower() or "folderview" in url.lower():
            if folder_match:
                return cls.get_folder_info(folder_match.group(1), url)

        # Check if single file
        file_match = cls.FILE_REGEX.search(url)
        if file_match and "/folders/" not in url.lower():
            try:
                return cls.get_file_info(file_match.group(1), url)
            except GoogleDriveFolderError:
                return cls.get_folder_info(file_match.group(1), url)

        # Fallback folder match if any
        if folder_match:
            return cls.get_folder_info(folder_match.group(1), url)

        raise ValueError("URL Google Drive tidak valid atau tidak dikenali.")

    @classmethod
    def get_folder_info(cls, folder_id: str, original_url: str) -> Dict[str, Any]:
        folder_page_url = cls._with_resource_key(f"https://drive.google.com/drive/folders/{folder_id}", original_url)
        session = requests.Session()
        
        try:
            res = session.get(folder_page_url, headers=cls.HEADERS, timeout=15)
        except Exception as e:
            raise RuntimeError(f"Gagal menghubungi server Google Drive: {str(e)}")

        if res.status_code >= 400:
            raise RuntimeError(f"Google Drive mengembalikan status {res.status_code}. Pastikan folder dibagikan secara publik ('Siapa saja yang memiliki link').")

        html = res.text

        # Extract folder title
        title_m = re.search(r'<title>(.*?)</title>', html, re.I | re.S)
        folder_title = "Folder Google Drive"
        if title_m:
            raw_title = title_m.group(1).strip()
            raw_title = re.sub(r'\s*-\s*Google Drive.*$', '', raw_title, flags=re.I).strip()
            folder_title = sanitize_filename(raw_title) or "Folder Google Drive"

        files = cls._extract_files_from_html(html)
        folders = [f for f in files if f.get("is_folder")]
        files = [f for f in files if not f.get("is_folder")]

        if not files and not folders:
            # Fallback check if it might be an empty folder or single file
            raise RuntimeError(f"Tidak ada berkas yang ditemukan dalam folder '{folder_title}'. Pastikan folder dapat diakses publik.")

        # Determine dominant category
        categories = [f.get("category", "General") for f in files]
        dominant_cat = max(set(categories), key=categories.count) if categories else "General"

        # Format list for UI
        formats = [
            {
                "format_id": "all",
                "quality": f"📦 Unduh Semua Berkas ({len(files)} Berkas)",
                "resolution": "Semua",
                "ext": "folder",
                "filesize": sum(f.get("filesize", 0) for f in files),
                "filesize_formatted": f"{len(files)} berkas",
                "has_video": any(f.get("category") == "Video" for f in files),
                "has_audio": any(f.get("category") == "Music" for f in files)
            }
        ]

        for f in files:
            ext = f["name"].split(".")[-1] if "." in f["name"] else "bin"
            formats.append({
                "format_id": f"file:{f['id']}",
                "quality": f"{f['name']}",
                "resolution": f.get("size_formatted") or "Berkas",
                "ext": ext,
                "filesize": f.get("filesize", 0),
                "filesize_formatted": f.get("size_formatted") or "Direct",
                "has_video": f.get("category") == "Video",
                "has_audio": f.get("category") == "Music",
                "download_url": f["download_url"]
            })

        first_thumb = files[0].get("thumbnail", "") if files else ""

        return {
            "is_folder": True,
            "is_stream": False,
            "is_gdrive": True,
            "url": folder_page_url,
            "folder_id": folder_id,
            "title": f"Folder: {folder_title}",
            "filename": f"{folder_title}.zip",
            "filesize": 0,
            "filesize_formatted": f"{len(files)} berkas, {len(folders)} folder",
            "category": dominant_cat,
            "thumbnail": first_thumb,
            "total_files": len(files),
            "files": files,
            "folders": folders,
            "formats": formats
        }

    @classmethod
    def _extract_files_from_html(cls, html: str) -> List[Dict[str, Any]]:
        found_files: List[Dict[str, Any]] = []

        # Find all AF_initDataCallback scripts
        callbacks = re.findall(r'AF_initDataCallback\((\{.*?\})\);', html, re.S)
        for cb_str in callbacks:
            data_idx = cb_str.find("data:")
            if data_idx == -1:
                continue

            # Look for clean data payload
            raw_data = cb_str[data_idx + 5:].strip()
            # Remove trailing sideChannel: {} or closing braces
            side_idx = raw_data.rfind(", sideChannel:")
            if side_idx != -1:
                raw_data = raw_data[:side_idx].strip()
            elif raw_data.endswith("}"):
                raw_data = raw_data[:-1].strip()

            try:
                data = json.loads(raw_data)
                cls._walk_and_extract_files(data, found_files)
            except Exception:
                pass

        # Deduplicate by file ID
        unique_map = {}
        for f in found_files:
            fid = f["id"]
            if fid not in unique_map:
                unique_map[fid] = f

        return list(unique_map.values())

    @classmethod
    def _walk_and_extract_files(cls, node: Any, out_files: List[Dict[str, Any]]) -> None:
        if isinstance(node, list):
            # Check if this node matches a Google Drive file entry
            # Pattern: len >= 35, node[0] is list with ID, node[4] is mime string
            if len(node) >= 35 and isinstance(node[0], list) and len(node[0]) >= 2 and isinstance(node[0][1], str):
                fid = node[0][1]
                if len(fid) >= 20 and len(fid) <= 50 and not any(c in fid for c in " /\\:?*\"<>|"):
                    mime = node[4] if len(node) > 4 and isinstance(node[4], str) else "application/octet-stream"
                    
                    # Filename extraction
                    name = None
                    if len(node) > 35 and isinstance(node[35], list) and node[35]:
                        try:
                            name = node[35][0][0][0]
                        except Exception:
                            pass
                    
                    if not name:
                        # Fallback scan for string with extension
                        for el in node:
                            if isinstance(el, str) and "." in el and len(el) > 3 and not el.startswith("http"):
                                name = el
                                break

                    if name and fid:
                        name = sanitize_filename(name)
                        # Extract human size string if present in node
                        size_str = cls._extract_size_string(node)
                        ext = name.split(".")[-1] if "." in name else "bin"
                        cat = determine_category(ext, mime)
                        
                        is_folder = mime == "application/vnd.google-apps.folder"
                        out_files.append({
                            "id": fid,
                            "name": name,
                            "mime": mime,
                            "is_folder": is_folder,
                            "category": cat,
                            "size_formatted": size_str,
                            "filesize": cls._parse_human_size(size_str),
                            "thumbnail": f"https://drive.google.com/thumbnail?id={fid}&sz=w320",
                            "download_url": (f"https://drive.google.com/drive/folders/{fid}" if is_folder else f"https://drive.usercontent.google.com/download?id={fid}&export=download&confirm=t")
                        })

            for child in node:
                cls._walk_and_extract_files(child, out_files)
        elif isinstance(node, dict):
            for v in node.values():
                cls._walk_and_extract_files(v, out_files)

    @classmethod
    def _extract_size_string(cls, node: Any) -> str:
        strings: List[str] = []
        def _get_strs(o):
            if isinstance(o, str):
                strings.append(o)
            elif isinstance(o, list):
                for x in o: _get_strs(x)
            elif isinstance(o, dict):
                for x in o.values(): _get_strs(x)
        _get_strs(node)

        for s in strings:
            m = re.search(r'(\d+(?:[.,]\d+)?\s*(?:KB|MB|GB|Bytes|B))\b', s, re.I)
            if m and "storage" not in s.lower():
                return m.group(1).replace(",", ".")
            elif m:
                return m.group(1).replace(",", ".")
        return ""

    @classmethod
    def _parse_human_size(cls, size_str: str) -> int:
        if not size_str:
            return 0
        m = re.match(r'([\d.]+)\s*([a-zA-Z]+)', size_str.strip())
        if not m:
            return 0
        num = float(m.group(1))
        unit = m.group(2).upper()
        if "GB" in unit:
            return int(num * 1024 * 1024 * 1024)
        if "MB" in unit:
            return int(num * 1024 * 1024)
        if "KB" in unit:
            return int(num * 1024)
        return int(num)

    @staticmethod
    def _with_resource_key(url: str, original_url: str) -> str:
        key = urllib.parse.parse_qs(urllib.parse.urlsplit(original_url).query).get("resourcekey", [])
        if key:
            url += ("&" if "?" in url else "?") + urllib.parse.urlencode({"resourcekey": key[0]})
        return url

    @classmethod
    def open_file(cls, file_id, original_url, session, headers=None):
        """Resolve Drive confirmation using the same session as the file transfer."""
        request_headers = {**cls.HEADERS, **(headers or {})}
        # A stale browser range must not turn metadata into a one-byte file.
        request_headers = {k: v for k, v in request_headers.items() if k.lower() != "range"}
        url = cls._with_resource_key(
            f"https://drive.usercontent.google.com/download?id={file_id}&export=download&confirm=t",
            original_url,
        )
        for _ in range(3):
            response = session.get(url, headers=request_headers, stream=True, allow_redirects=True, timeout=25)
            content_type = response.headers.get("Content-Type", "").lower()
            disposition = response.headers.get("Content-Disposition", "")
            if response.status_code < 400 and (disposition or "text/html" not in content_type):
                return response
            body = response.raw.read(262144).decode("utf-8", errors="replace")
            status = response.status_code
            page_url = response.url
            response.close()
            if status == 404:
                with session.get(
                    cls._with_resource_key(f"https://drive.google.com/file/d/{file_id}/view", original_url),
                    headers=request_headers, timeout=15,
                ) as view:
                    if 'application/vnd.google-apps.folder' in view.text:
                        raise GoogleDriveFolderError(
                            "Tautan ini adalah folder Google Drive, bukan berkas. "
                            f"Tambahkan https://drive.google.com/drive/folders/{file_id} "
                            "lalu buka subfolder dan pilih berkas yang ingin diunduh."
                        )
            lower_body = body.lower()
            if any(text in lower_body for text in ("download quota", "too many users", "quota exceeded")):
                raise RuntimeError("Kuota unduhan Google Drive habis. Coba lagi nanti.")
            parser = _DownloadFormParser()
            parser.feed(body)
            next_url = None
            if status < 400 and parser.action and "confirm" in parser.inputs:
                action = urllib.parse.urlsplit(urllib.parse.urljoin(page_url, parser.action))
                params = dict(urllib.parse.parse_qsl(action.query))
                params.update(parser.inputs)
                next_url = urllib.parse.urlunsplit(action._replace(query=urllib.parse.urlencode(params)))
            elif status < 400:
                match = re.search(r'href=[\'"]([^\'"]*(?:/uc\?|/download\?)[^\'"]+)[\'"]', body)
                if match:
                    next_url = urllib.parse.urljoin(page_url, unescape(match.group(1)))
            if next_url:
                host = urllib.parse.urlsplit(next_url).hostname or ""
                if host in ("drive.google.com", "drive.usercontent.google.com", "docs.google.com"):
                    url = next_url
                    continue
            raise RuntimeError(
                f"Google Drive tidak mengirim berkas (HTTP {status}). "
                "Periksa akses berbagi, izin download, dan pastikan tautan menunjuk ke berkas."
            )
        raise RuntimeError("Konfirmasi unduhan Google Drive gagal. Coba analisis ulang tautan berkas.")

    @classmethod
    def file_metadata(cls, response, file_id, original_url):
        disposition = response.headers.get("Content-Disposition", "")
        extended = re.search(r"filename\*=([^;]+)", disposition, re.I)
        regular = re.search(r'filename="([^"]+)"|filename=([^;]+)', disposition, re.I)
        filename = ""
        if extended:
            value = extended.group(1).strip().strip('"')
            filename = urllib.parse.unquote(value.split("'", 2)[-1])
        elif regular:
            filename = regular.group(1) or regular.group(2).strip()
        filename = sanitize_filename(unescape(filename)) if filename else f"GoogleDrive_File_{file_id}"
        size = int(response.headers.get("Content-Length", 0))
        category = determine_category(filename.rsplit(".", 1)[-1], response.headers.get("Content-Type", ""), original_url)
        return {
            "is_stream": False, "is_folder": False, "is_gdrive": True,
            # Keep the stable Drive URL; confirmation URLs/cookies can expire.
            "url": cls._with_resource_key(f"https://drive.usercontent.google.com/download?id={file_id}&export=download&confirm=t", original_url),
            "title": filename, "filename": filename,
            "filesize": size, "filesize_formatted": format_bytes(size) if size else "Unknown size",
            "accept_ranges": "bytes" in response.headers.get("Accept-Ranges", "").lower(),
            "category": category,
            "thumbnail": f"https://drive.google.com/thumbnail?id={file_id}&sz=w320",
            "formats": [{
                "format_id": "direct", "quality": "Google Drive Direct File", "resolution": "Original",
                "ext": filename.rsplit(".", 1)[-1] if "." in filename else "bin",
                "filesize": size, "filesize_formatted": format_bytes(size) if size else "Direct",
                "has_video": category == "Video", "has_audio": category == "Music",
            }],
        }

    @classmethod
    def get_file_info(cls, file_id: str, original_url: str) -> Dict[str, Any]:
        with requests.Session() as session:
            with cls.open_file(file_id, original_url, session) as response:
                return cls.file_metadata(response, file_id, original_url)
