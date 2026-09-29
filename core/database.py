import sqlite3
import time
from typing import List, Dict, Optional, Any
from .config import DB_PATH

def get_connection() -> sqlite3.Connection:
    conn = sqlite3.connect(str(DB_PATH), check_same_thread=False)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode = WAL")
    conn.execute("PRAGMA synchronous = NORMAL")
    return conn

def init_db() -> None:
    with get_connection() as conn:
        conn.execute("""
            CREATE TABLE IF NOT EXISTS downloads (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                title TEXT NOT NULL,
                filename TEXT NOT NULL,
                filepath TEXT NOT NULL,
                filesize INTEGER DEFAULT 0,
                downloaded_bytes INTEGER DEFAULT 0,
                progress REAL DEFAULT 0.0,
                speed REAL DEFAULT 0.0,
                eta INTEGER DEFAULT 0,
                status TEXT DEFAULT 'pending', -- pending, downloading, paused, completed, error
                category TEXT DEFAULT 'General', -- Video, Music, Documents, Programs, Compressed, General
                thumbnail TEXT DEFAULT '',
                format_id TEXT DEFAULT '',
                error_message TEXT DEFAULT '',
                created_at REAL NOT NULL,
                completed_at REAL DEFAULT NULL
            )
        """)
        conn.execute("CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status)")
        conn.execute("CREATE INDEX IF NOT EXISTS idx_downloads_category ON downloads(category)")
        conn.execute("CREATE INDEX IF NOT EXISTS idx_downloads_created ON downloads(created_at DESC)")

class DatabaseManager:
    def __init__(self):
        init_db()

    def add_download(self, item: Dict[str, Any]) -> None:
        with get_connection() as conn:
            conn.execute("""
                INSERT OR REPLACE INTO downloads (
                    id, url, title, filename, filepath, filesize, downloaded_bytes,
                    progress, speed, eta, status, category, thumbnail, format_id,
                    error_message, created_at, completed_at
                ) VALUES (
                    :id, :url, :title, :filename, :filepath, :filesize, :downloaded_bytes,
                    :progress, :speed, :eta, :status, :category, :thumbnail, :format_id,
                    :error_message, :created_at, :completed_at
                )
            """, {
                "id": item["id"],
                "url": item["url"],
                "title": item.get("title", "Untitled"),
                "filename": item.get("filename", "unknown_file"),
                "filepath": item.get("filepath", ""),
                "filesize": item.get("filesize", 0),
                "downloaded_bytes": item.get("downloaded_bytes", 0),
                "progress": item.get("progress", 0.0),
                "speed": item.get("speed", 0.0),
                "eta": item.get("eta", 0),
                "status": item.get("status", "pending"),
                "category": item.get("category", "General"),
                "thumbnail": item.get("thumbnail", ""),
                "format_id": item.get("format_id", ""),
                "error_message": item.get("error_message", ""),
                "created_at": item.get("created_at", time.time()),
                "completed_at": item.get("completed_at", None)
            })

    def update_progress(self, download_id: str, downloaded_bytes: int, filesize: int, speed: float, eta: int, progress: float) -> None:
        with get_connection() as conn:
            conn.execute("""
                UPDATE downloads
                SET downloaded_bytes = ?, filesize = ?, speed = ?, eta = ?, progress = ?
                WHERE id = ?
            """, (downloaded_bytes, filesize, speed, eta, progress, download_id))

    def update_status(self, download_id: str, status: str, error_message: str = "", completed_at: Optional[float] = None) -> None:
        with get_connection() as conn:
            conn.execute("""
                UPDATE downloads
                SET status = ?, error_message = ?, completed_at = COALESCE(?, completed_at)
                WHERE id = ?
            """, (status, error_message, completed_at, download_id))

    def update_file_info(self, download_id: str, filepath: str, filename: str, filesize: int) -> None:
        with get_connection() as conn:
            conn.execute("""
                UPDATE downloads
                SET filepath = ?, filename = ?, filesize = ?
                WHERE id = ?
            """, (filepath, filename, filesize, download_id))

    def get_all(self, category: Optional[str] = None, status: Optional[str] = None) -> List[Dict[str, Any]]:
        query = "SELECT * FROM downloads WHERE 1=1"
        params = []
        if category and category.lower() != "all":
            query += " AND category = ?"
            params.append(category)
        if status and status.lower() != "all":
            query += " AND status = ?"
            params.append(status)
        query += " ORDER BY created_at DESC"

        with get_connection() as conn:
            cursor = conn.execute(query, params)
            return [dict(row) for row in cursor.fetchall()]

    def get_by_id(self, download_id: str) -> Optional[Dict[str, Any]]:
        with get_connection() as conn:
            cursor = conn.execute("SELECT * FROM downloads WHERE id = ?", (download_id,))
            row = cursor.fetchone()
            return dict(row) if row else None

    def delete_by_id(self, download_id: str) -> bool:
        with get_connection() as conn:
            cursor = conn.execute("DELETE FROM downloads WHERE id = ?", (download_id,))
            return cursor.rowcount > 0
