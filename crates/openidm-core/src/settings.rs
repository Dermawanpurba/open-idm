//! User settings, stored as `settings.json` next to the executable.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "settings.json";

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub max_concurrent: usize,
    pub segments: usize,
    pub autostart: bool,
    pub confirm_dialog: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"));
        Settings {
            download_dir: home.join("Downloads").join("OpenIDM"),
            max_concurrent: 3,
            segments: 8,
            autostart: true,
            confirm_dialog: true,
        }
    }
}

impl Settings {
    /// A missing or unreadable file yields the defaults.
    pub fn load(data_dir: &Path) -> Settings {
        std::fs::read(data_dir.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .and_then(|s| s.validated().ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> io::Result<()> {
        let tmp = data_dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, data_dir.join(FILE))
    }

    pub fn validated(self) -> Result<Settings, String> {
        if !(1..=8).contains(&self.max_concurrent) {
            return Err("Jumlah unduhan bersamaan harus antara 1 dan 8.".into());
        }
        if !(1..=16).contains(&self.segments) {
            return Err("Jumlah koneksi per unduhan harus antara 1 dan 16.".into());
        }
        if !self.download_dir.is_absolute() {
            return Err("Folder unduhan harus berupa path lengkap, misalnya D:\\Unduhan.".into());
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_roundtrip_and_validation() {
        let d = tempfile::tempdir().unwrap();
        let s = Settings::load(d.path());
        assert_eq!((s.max_concurrent, s.segments, s.autostart, s.confirm_dialog), (3, 8, true, true));
        assert!(s.download_dir.ends_with("Downloads/OpenIDM"));
        let mut t = s.clone();
        t.max_concurrent = 2;
        t.save(d.path()).unwrap();
        assert_eq!(Settings::load(d.path()), t);
        std::fs::write(d.path().join("settings.json"), "{broken").unwrap();
        assert_eq!(Settings::load(d.path()), Settings::default());
        assert!(Settings { max_concurrent: 0, ..Settings::default() }.validated().is_err());
        assert!(Settings { max_concurrent: 9, ..Settings::default() }.validated().is_err());
        assert!(Settings { segments: 17, ..Settings::default() }.validated().is_err());
        assert!(Settings { download_dir: "relative".into(), ..Settings::default() }.validated().is_err());
        assert!(Settings::default().validated().is_ok());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("settings.json"), r#"{"autostart":false}"#).unwrap();
        let s = Settings::load(d.path());
        assert!(!s.autostart);
        assert_eq!(s.segments, 8);
    }
}
