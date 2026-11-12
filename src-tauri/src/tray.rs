//! System tray icon and the "bring the main window forward" helper.

use std::sync::Arc;

use openidm_core::manager::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _};

/// Shows, restores and focuses the main window.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id("open", "Buka OpenIDM").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("pause_all", "Jeda semua").build(app)?)
        .item(&MenuItemBuilder::with_id("resume_all", "Lanjutkan semua").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("quit", "Keluar").build(app)?)
        .build()?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("OpenIDM")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let manager = app.state::<Arc<Manager>>().inner().clone();
            match event.id().as_ref() {
                "open" => show_main(app),
                "pause_all" => manager.pause_all(),
                "resume_all" => manager.resume_all(),
                "quit" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(crate::commands::quit(app, manager));
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
