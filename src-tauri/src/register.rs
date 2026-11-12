//! Registers the Native Messaging host for Chrome and Edge (current user only, no admin rights).

use std::path::Path;

use openidm_core::protocol::{EXTENSION_ID, HOST_NAME};
use serde::Serialize;
use serde_json::{json, Value};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const MANIFEST_FILE: &str = "openidm-host.json";
const HOST_EXE: &str = "openidm-host.exe";

/// Registry locations Chrome and Edge read (under HKEY_CURRENT_USER).
const BROWSERS: [(&str, &str); 2] = [
    ("chrome", r"Software\Google\Chrome\NativeMessagingHosts\com.openidm.host"),
    ("edge", r"Software\Microsoft\Edge\NativeMessagingHosts\com.openidm.host"),
];

#[derive(Serialize, Debug, Default)]
pub struct BrowserStatus {
    pub chrome: bool,
    pub edge: bool,
    /// False when `openidm-host.exe` is missing next to the app.
    pub host_exists: bool,
}

/// The host manifest: only the OpenIDM extension may talk to the host.
pub fn host_manifest(exe_dir: &Path) -> Value {
    json!({
        "name": HOST_NAME,
        "description": "OpenIDM download manager",
        "path": exe_dir.join(HOST_EXE),
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    })
}

pub fn register_in(exe_dir: &Path, browsers: &[(&str, &str)]) -> Result<(), String> {
    let failed = || "Integrasi browser tidak bisa didaftarkan.".to_string();
    let manifest = exe_dir.join(MANIFEST_FILE);
    let body = serde_json::to_vec_pretty(&host_manifest(exe_dir)).map_err(|_| failed())?;
    std::fs::write(&manifest, body).map_err(|_| failed())?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for (_, key) in browsers {
        let (key, _) = hkcu.create_subkey(key).map_err(|_| failed())?;
        key.set_value("", &manifest.to_string_lossy().into_owned()).map_err(|_| failed())?;
    }
    Ok(())
}

pub fn status_in(exe_dir: &Path, browsers: &[(&str, &str)]) -> BrowserStatus {
    let expected = exe_dir.join(MANIFEST_FILE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let registered = |name: &str| {
        browsers.iter().filter(|(browser, _)| *browser == name).any(|(_, key)| {
            hkcu.open_subkey(key)
                .and_then(|k| k.get_value::<String, _>(""))
                .map(|value| Path::new(&value) == expected && expected.is_file())
                .unwrap_or(false)
        })
    };
    BrowserStatus { chrome: registered("chrome"), edge: registered("edge"), host_exists: exe_dir.join(HOST_EXE).is_file() }
}

/// Points Chrome and Edge at this folder's host. Run at every start so a moved folder keeps working.
pub fn register(exe_dir: &Path) -> Result<(), String> {
    register_in(exe_dir, &BROWSERS)
}

pub fn status(exe_dir: &Path) -> BrowserStatus {
    status_in(exe_dir, &BROWSERS)
}

#[cfg(test)]
mod tests {
    use openidm_core::protocol::EXTENSION_ID;
    use serde_json::json;

    use super::*;

    #[test]
    fn manifest_content() {
        let d = tempfile::tempdir().unwrap();
        let v = host_manifest(d.path());
        assert_eq!(v["name"], "com.openidm.host");
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["allowed_origins"], json!([format!("chrome-extension://{EXTENSION_ID}/")]));
        let path = std::path::Path::new(v["path"].as_str().unwrap());
        assert!(path.is_absolute());
        assert_eq!(path, d.path().join("openidm-host.exe"));
    }

    #[test]
    fn register_writes_manifest_and_status_reflects_registry() {
        let d = tempfile::tempdir().unwrap();
        // A private registry root keeps the test away from the real browser keys.
        let root = format!(r"Software\OpenIDM-Test-{}", std::process::id());
        let browsers = [("chrome", format!(r"{root}\Chrome")), ("edge", format!(r"{root}\Edge"))];
        let keys: Vec<(&str, &str)> = browsers.iter().map(|(n, k)| (*n, k.as_str())).collect();

        let before = status_in(d.path(), &keys);
        assert_eq!((before.chrome, before.edge, before.host_exists), (false, false, false));

        register_in(d.path(), &keys).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(d.path().join("openidm-host.json")).unwrap()).unwrap();
        assert_eq!(manifest, host_manifest(d.path()));

        let after = status_in(d.path(), &keys);
        assert_eq!((after.chrome, after.edge), (true, true));
        assert!(!after.host_exists, "openidm-host.exe is not in the temp folder");
        std::fs::write(d.path().join("openidm-host.exe"), b"x").unwrap();
        assert!(status_in(d.path(), &keys).host_exists);

        // A registration pointing at another folder (app moved) is reported as not registered.
        let moved = tempfile::tempdir().unwrap();
        assert!(!status_in(moved.path(), &keys).chrome);

        winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).delete_subkey_all(&root).unwrap();
    }
}
