//! Registers the Native Messaging host for Chrome and Edge (current user only, no admin rights).

// IMPL

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
