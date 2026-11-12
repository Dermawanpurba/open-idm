#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod capture;
mod commands;
mod pipe;
mod register;
mod tray;

use std::path::PathBuf;

use openidm_core::manager::Manager;
use tauri::{AppHandle, Manager as _, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Data (database, settings) lives next to the executable: the app is portable.
pub fn data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Makes the Windows "run at sign-in" entry match the setting.
pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    let autolaunch = app.autolaunch();
    if autolaunch.is_enabled().unwrap_or(false) != enabled {
        let _ = if enabled { autolaunch.enable() } else { autolaunch.disable() };
    }
}

fn main() {
    let background = std::env::args().any(|arg| arg == "--background");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--background"])))
        .manage(bridge::Confirms::default())
        .setup(move |app| {
            let dir = data_dir();
            let manager = tauri::async_runtime::block_on(async { Manager::open(&dir) })?;
            sync_autostart(app.handle(), manager.settings().autostart);
            app.manage(manager.clone());
            tray::build(app.handle())?;
            // Re-register on every start so a moved folder keeps working. Failure is shown in Pengaturan.
            let _ = register::register(&dir);
            tauri::async_runtime::spawn(bridge::serve(app.handle().clone(), manager));
            if !background {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the main window only hides it; downloads keep running in the tray.
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Destroyed => bridge::on_confirm_window_gone(window.app_handle(), window.label()),
            _ => {}
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
            commands::browser_status,
            commands::register_browser,
            bridge::confirm_get,
            bridge::confirm_decide,
        ])
        .run(tauri::generate_context!())
        .expect("OpenIDM failed to start");
}
