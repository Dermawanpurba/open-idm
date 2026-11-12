# Active development: portable Rust

- Develop the Rust/Tauri application in this repository. The user discontinued the Python portable package.
- `crates/openidm-core`, `crates/openidm-host`, `src-tauri`, `ui`, and `browser-extension` are the active implementation.
- Python files are migration references only. Do not restore Python/PyInstaller packaging unless the user explicitly requests it.
- The canonical portable output is `C:\xampp\htdocs\open-idm`; `D:\rust\run-test` is a separate test instance and its data must be preserved.
- Use `build-rust.bat` (or the `build.bat` wrapper) for Rust tests, extension tests, release build, and packaging. The first argument overrides the output folder.
- Do not stop an instance from another folder merely to package a different destination.
- Preserve `downloads.db`, `settings.json`, and downloaded files when replacing packages. Legacy user data in `.backups/` is not build input or distribution content.
