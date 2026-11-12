//! SQLite storage. Same file and schema as the Python core so existing history stays readable.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// One download. Status is one of: pending, downloading, paused, completed, error.
#[derive(Serialize, Clone, Debug)]
pub struct TaskRow {
    pub id: String,
    pub url: String,
    pub title: String,
    pub filename: String,
    pub filepath: String,
    pub filesize: u64,
    pub downloaded_bytes: u64,
    pub progress: f64,
    pub speed: f64,
    pub eta: u64,
    pub status: String,
    pub category: String,
    pub thumbnail: String,
    pub format_id: String,
    pub error_message: String,
    pub request_id: String,
    pub created_at: f64,
    pub completed_at: Option<f64>,
    /// Never sent to the UI: may hold the browser's cookies.
    #[serde(skip)]
    pub headers: BTreeMap<String, String>,
    /// Not stored; true unless the manager knows the server cannot resume.
    pub can_resume: bool,
}

const COLUMNS: &str = "id, url, title, filename, filepath, filesize, downloaded_bytes, progress, speed, eta, \
                       status, category, thumbnail, format_id, error_message, request_id, created_at, \
                       completed_at, headers";

fn from_row(row: &Row) -> rusqlite::Result<TaskRow> {
    let text = |i: usize| row.get::<_, Option<String>>(i).map(Option::unwrap_or_default);
    let int = |i: usize| row.get::<_, Option<i64>>(i).map(|v| v.unwrap_or(0).max(0) as u64);
    let real = |i: usize| row.get::<_, Option<f64>>(i).map(Option::unwrap_or_default);
    Ok(TaskRow {
        id: text(0)?,
        url: text(1)?,
        title: text(2)?,
        filename: text(3)?,
        filepath: text(4)?,
        filesize: int(5)?,
        downloaded_bytes: int(6)?,
        progress: real(7)?,
        speed: real(8)?,
        eta: int(9)?,
        status: text(10)?,
        category: text(11)?,
        thumbnail: text(12)?,
        format_id: text(13)?,
        error_message: text(14)?,
        request_id: text(15)?,
        created_at: real(16)?,
        completed_at: row.get(17)?,
        headers: serde_json::from_str(&text(18)?).unwrap_or_default(),
        can_resume: true,
    })
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Opens (or creates) the database and brings an older schema up to date.
    pub fn open(path: &Path) -> rusqlite::Result<Db> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS downloads (
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
                status TEXT DEFAULT 'pending',
                category TEXT DEFAULT 'General',
                thumbnail TEXT DEFAULT '',
                format_id TEXT DEFAULT '',
                headers TEXT DEFAULT '',
                error_message TEXT DEFAULT '',
                created_at REAL NOT NULL,
                completed_at REAL DEFAULT NULL
            )",
        )?;
        let existing: Vec<String> = conn
            .prepare("PRAGMA table_info(downloads)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        for column in ["headers", "format_id", "request_id"] {
            if !existing.iter().any(|c| c == column) {
                conn.execute_batch(&format!("ALTER TABLE downloads ADD COLUMN {column} TEXT DEFAULT ''"))?;
            }
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);
             CREATE INDEX IF NOT EXISTS idx_downloads_category ON downloads(category);
             CREATE INDEX IF NOT EXISTS idx_downloads_created ON downloads(created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_downloads_request_id ON downloads(request_id);",
        )?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)) })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn insert(&self, row: &TaskRow) -> rusqlite::Result<()> {
        let headers = serde_json::to_string(&row.headers).unwrap_or_else(|_| "{}".into());
        self.conn().execute(
            &format!(
                "INSERT INTO downloads ({COLUMNS}) VALUES \
                 (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)"
            ),
            params![
                row.id, row.url, row.title, row.filename, row.filepath, row.filesize as i64,
                row.downloaded_bytes as i64, row.progress, row.speed, row.eta as i64, row.status, row.category,
                row.thumbnail, row.format_id, row.error_message, row.request_id, row.created_at,
                row.completed_at, headers
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> rusqlite::Result<Option<TaskRow>> {
        self.conn()
            .query_row(&format!("SELECT {COLUMNS} FROM downloads WHERE id = ?1"), [id], from_row)
            .optional()
    }

    /// Newest first, optionally filtered.
    pub fn list(&self, category: Option<&str>, status: Option<&str>) -> rusqlite::Result<Vec<TaskRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM downloads \
             WHERE (?1 IS NULL OR category = ?1) AND (?2 IS NULL OR status = ?2) \
             ORDER BY created_at DESC"
        ))?;
        let rows = stmt.query_map(params![category, status], from_row)?;
        rows.collect()
    }

    pub fn set_status(&self, id: &str, status: &str, error: &str) -> rusqlite::Result<()> {
        let completed_at = (status == "completed").then(now);
        self.conn().execute(
            "UPDATE downloads SET status = ?2, error_message = ?3, \
             completed_at = COALESCE(?4, completed_at), speed = 0, eta = 0 WHERE id = ?1",
            params![id, status, error, completed_at],
        )?;
        Ok(())
    }

    pub fn set_progress(&self, id: &str, downloaded: u64, filesize: u64, speed: f64, eta: u64) -> rusqlite::Result<()> {
        let progress = if filesize > 0 { (downloaded as f64 / filesize as f64 * 100.0).min(100.0) } else { 0.0 };
        self.conn().execute(
            "UPDATE downloads SET downloaded_bytes = ?2, filesize = ?3, speed = ?4, eta = ?5, progress = ?6 \
             WHERE id = ?1",
            params![id, downloaded as i64, filesize as i64, speed, eta as i64, progress],
        )?;
        Ok(())
    }

    pub fn set_file(&self, id: &str, filepath: &str, filename: &str, filesize: u64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE downloads SET filepath = ?2, filename = ?3, filesize = ?4, downloaded_bytes = ?4, \
             progress = 100.0 WHERE id = ?1",
            params![id, filepath, filename, filesize as i64],
        )?;
        Ok(())
    }

    /// Drops the stored browser cookies once a download no longer needs them.
    pub fn clear_cookies(&self, id: &str) -> rusqlite::Result<()> {
        let Some(mut row) = self.get(id)? else { return Ok(()) };
        row.headers.remove("Cookie");
        let headers = serde_json::to_string(&row.headers).unwrap_or_else(|_| "{}".into());
        self.conn().execute("UPDATE downloads SET headers = ?2 WHERE id = ?1", params![id, headers])?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> rusqlite::Result<bool> {
        Ok(self.conn().execute("DELETE FROM downloads WHERE id = ?1", [id])? > 0)
    }

    /// Called at startup: nothing is actually running yet, so mark leftovers as paused.
    pub fn reset_running_to_paused(&self) -> rusqlite::Result<usize> {
        self.conn().execute(
            "UPDATE downloads SET status = 'paused', speed = 0, eta = 0 WHERE status IN ('downloading', 'pending')",
            [],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, status: &str, created_at: f64) -> TaskRow {
        TaskRow {
            id: id.into(),
            url: format!("https://a.com/{id}.zip"),
            title: id.into(),
            filename: format!("{id}.zip"),
            filepath: format!("C:/x/{id}.zip"),
            filesize: 0,
            downloaded_bytes: 0,
            progress: 0.0,
            speed: 0.0,
            eta: 0,
            status: status.into(),
            category: "Compressed".into(),
            thumbnail: String::new(),
            format_id: "direct".into(),
            error_message: String::new(),
            request_id: String::new(),
            created_at,
            completed_at: None,
            headers: [("Cookie", "c"), ("Referer", "r")].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            can_resume: true,
        }
    }

    #[test]
    fn opens_legacy_db_and_adds_request_id() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("downloads.db");
        {
            let c = rusqlite::Connection::open(&p).unwrap();
            c.execute_batch(
                "CREATE TABLE downloads (id TEXT PRIMARY KEY, url TEXT NOT NULL, title TEXT NOT NULL, filename TEXT NOT NULL, filepath TEXT NOT NULL, filesize INTEGER DEFAULT 0, downloaded_bytes INTEGER DEFAULT 0, progress REAL DEFAULT 0.0, speed REAL DEFAULT 0.0, eta INTEGER DEFAULT 0, status TEXT DEFAULT 'pending', category TEXT DEFAULT 'General', thumbnail TEXT DEFAULT '', format_id TEXT DEFAULT '', error_message TEXT DEFAULT '', created_at REAL NOT NULL, completed_at REAL DEFAULT NULL, headers TEXT DEFAULT '');
                 INSERT INTO downloads (id,url,title,filename,filepath,status,created_at,headers) VALUES ('old1','https://a/b','T','b.webm','C:/x/b.webm','completed',1.0,'{\"Cookie\":\"s\"}');",
            )
            .unwrap();
        }
        let db = Db::open(&p).unwrap();
        let r = db.get("old1").unwrap().unwrap();
        assert_eq!((r.status.as_str(), r.request_id.as_str()), ("completed", ""));
        assert_eq!(r.headers["Cookie"], "s");
    }

    #[test]
    fn lifecycle() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("x.db")).unwrap();
        db.insert(&row("a", "downloading", 1.0)).unwrap();
        db.insert(&row("b", "pending", 2.0)).unwrap();
        assert_eq!(db.list(None, None).unwrap().iter().map(|r| r.id.clone()).collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(db.list(None, Some("pending")).unwrap().len(), 1);
        assert_eq!(db.list(Some("Video"), None).unwrap().len(), 0);
        db.set_progress("a", 50, 200, 10.0, 15).unwrap();
        let a = db.get("a").unwrap().unwrap();
        assert_eq!((a.downloaded_bytes, a.filesize, a.progress), (50, 200, 25.0));
        assert_eq!(db.reset_running_to_paused().unwrap(), 2);
        db.set_status("a", "completed", "").unwrap();
        assert!(db.get("a").unwrap().unwrap().completed_at.is_some());
        db.clear_cookies("a").unwrap();
        let headers = db.get("a").unwrap().unwrap().headers;
        assert!(!headers.contains_key("Cookie"));
        assert_eq!(headers["Referer"], "r");
        assert!(db.delete("a").unwrap());
        assert!(!db.delete("a").unwrap());
    }

    #[test]
    fn malformed_headers_column_reads_as_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.db");
        let db = Db::open(&p).unwrap();
        db.insert(&row("a", "paused", 1.0)).unwrap();
        rusqlite::Connection::open(&p).unwrap().execute("UPDATE downloads SET headers = 'not json'", []).unwrap();
        assert!(db.get("a").unwrap().unwrap().headers.is_empty());
    }
}
