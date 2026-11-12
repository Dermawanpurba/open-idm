#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod commands;
mod register;
mod tray;

use std::path::PathBuf;

use openidm_core::manager::Manager;
use tauri::{AppHandle, Manager as _, WindowEvent};

/// Data (database, settings) lives next to the executable: the app is portable.
fn data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn sync_autostart(_app: &AppHandle, _enabled: bool) {}

fn main() {
    let background = std::env::args().any(|arg| arg == "--background");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let dir = data_dir();
            let manager = tauri::async_runtime::block_on(async { Manager::open(&dir) })?;
            app.manage(manager);
            tray::build(app.handle())?;
            if !background {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window only hides it; downloads keep running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::list_downloads,
            commands::add_download,
            commands::control_download,
            commands::open_file,
            commands::open_folder,
            commands::open_download_dir,
            commands::get_info,
            commands::get_settings,
            commands::save_settings,
            commands::pick_folder,
            commands::quit_app,
            commands::win_minimize,
            commands::win_toggle_maximize,
            commands::win_hide,
        ])
        .run(tauri::generate_context!())
        .expect("OpenIDM failed to start");
}
