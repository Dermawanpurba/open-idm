import re
import time
import json
import urllib.parse
from typing import Dict, Any, List, Optional, Tuple
import requests

from .stream_extractor import format_bytes, sanitize_filename, determine_category

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
            return cls.get_file_info(file_match.group(1), url)

        # Fallback folder match if any
        if folder_match:
            return cls.get_folder_info(folder_match.group(1), url)

        raise ValueError("URL Google Drive tidak valid atau tidak dikenali.")

    @classmethod
    def get_folder_info(cls, folder_id: str, original_url: str) -> Dict[str, Any]:
        folder_page_url = f"https://drive.google.com/drive/folders/{folder_id}"
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

        if not files:
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
            "filesize_formatted": f"{len(files)} berkas",
            "category": dominant_cat,
            "thumbnail": first_thumb,
            "total_files": len(files),
            "files": files,
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
                        
                        out_files.append({
                            "id": fid,
                            "name": name,
                            "mime": mime,
                            "category": cat,
                            "size_formatted": size_str,
                            "filesize": cls._parse_human_size(size_str),
                            "thumbnail": f"https://drive.google.com/thumbnail?id={fid}&sz=w320",
                            "download_url": f"https://drive.usercontent.google.com/download?id={fid}&export=download&confirm=t"
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

    @classmethod
    def get_file_info(cls, file_id: str, original_url: str) -> Dict[str, Any]:
        direct_download_url = f"https://drive.usercontent.google.com/download?id={file_id}&export=download&confirm=t"
        session = requests.Session()

        try:
            r = session.get(direct_download_url, headers=cls.HEADERS, stream=True, allow_redirects=True, timeout=12)
        except Exception as e:
            raise RuntimeError(f"Gagal menghubungi Google Drive: {str(e)}")

        ct = r.headers.get("Content-Type", "").lower()
        # If virus scan html warning returned, follow confirm token
        if "text/html" in ct:
            body = r.raw.read(65536).decode("utf-8", errors="ignore")
            confirm_m = re.search(r'href=[\'"](/uc\?export=download[^\'"]+)[\'"]', body)
            if confirm_m:
                c_url = "https://drive.google.com" + confirm_m.group(1).replace("&amp;", "&")
                r = session.get(c_url, headers=cls.HEADERS, stream=True, allow_redirects=True, timeout=12)
            else:
                action = re.search(r'<form[^>]+action=[\'"]([^\'"]+)[\'"]', body)
                uuid_m = re.search(r'name=[\'"]uuid[\'"]\s+value=[\'"]([^\'"]+)[\'"]', body)
                confirm_token = re.search(r'name=[\'"]confirm[\'"]\s+value=[\'"]([^\'"]+)[\'"]', body)
                if action and confirm_token:
                    act = action.group(1)
                    u = uuid_m.group(1) if uuid_m else ""
                    c = confirm_token.group(1)
                    direct = f"{act}?id={file_id}&export=download&confirm={c}" + (f"&uuid={u}" if u else "")
                    r = session.get(direct, headers=cls.HEADERS, stream=True, allow_redirects=True, timeout=12)

        # Extract filename
        cd = r.headers.get("Content-Disposition", "")
        filename = ""
        if cd and "filename=" in cd:
            fm = re.search(r'filename\*?=(?:UTF-8\'\')?"?([^";]+)"?', cd)
            if fm:
                filename = urllib.parse.unquote(fm.group(1))

        if not filename:
            # Try fetching view page title
            try:
                view_res = session.get(f"https://drive.google.com/file/d/{file_id}/view", headers=cls.HEADERS, timeout=8)
                vm = re.search(r'<title>(.*?)</title>', view_res.text, re.I)
                if vm:
                    raw_title = vm.group(1).strip()
                    raw_title = re.sub(r'\s*-\s*Google Drive.*$', '', raw_title, flags=re.I).strip()
                    filename = sanitize_filename(raw_title)
            except Exception:
                pass

        filename = filename or f"GoogleDrive_File_{file_id}"
        filename = sanitize_filename(filename)
        cl = int(r.headers.get("Content-Length", 0))
        final_url = r.url
        r.close()

        ext = filename.split(".")[-1] if "." in filename else "bin"
        cat = determine_category(ext, r.headers.get("Content-Type", ""), original_url)

        return {
            "is_stream": False,
            "is_folder": False,
            "is_gdrive": True,
            "url": final_url,
            "title": filename,
            "filename": filename,
            "filesize": cl,
            "filesize_formatted": format_bytes(cl) if cl > 0 else "Unknown size",
            "accept_ranges": True,
            "category": cat,
            "thumbnail": f"https://drive.google.com/thumbnail?id={file_id}&sz=w320",
            "formats": [
                {
                    "format_id": "direct",
                    "quality": "Google Drive Direct File",
                    "resolution": "Original",
                    "ext": ext,
                    "filesize": cl,
                    "filesize_formatted": format_bytes(cl) if cl > 0 else "Direct",
                    "has_video": cat == "Video",
                    "has_audio": cat == "Music"
                }
            ]
        }
