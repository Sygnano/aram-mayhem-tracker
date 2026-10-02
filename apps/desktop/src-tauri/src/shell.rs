//! The app's place on the desktop: the tray icon, starting with Windows, and how the companion
//! window comes up.

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::engine::Engine;

const TRAY_ID: &str = "main";
/// Passed on the command line by the Windows start-up entry, and by nothing else: a launch by hand
/// always opens the window.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Builds the tray icon, brings the start-up entry in line with the config, and opens the companion
/// window. The window is created hidden (`tauri.conf.json`) so that a minimised start never flashes
/// it on screen first.
pub fn setup(app: &AppHandle, engine: &Arc<Engine>) -> tauri::Result<()> {
    let (keep_in_tray, start_minimized) = {
        let st = engine.lock();
        (st.settings.keep_in_tray, st.settings.start_minimized)
    };

    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("ARAM Mayhem Tracker")
        .menu(&Menu::with_items(app, &[&open, &quit])?)
        // A left click opens the window; the menu is the right click's.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_companion(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_companion(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    // Always built, shown only while the setting is on, so ticking the box needs no restart.
    set_tray_visible(app, keep_in_tray);

    if let Err(e) = sync_autostart(app, start_minimized) {
        log::warn!("start with Windows: {e}");
    }

    if let Some(w) = app.get_webview_window("companion") {
        if !std::env::args().any(|a| a == MINIMIZED_ARG) {
            w.show()?;
        } else if !keep_in_tray {
            // No tray icon to come back from, so it goes to the taskbar instead.
            w.minimize()?;
            w.show()?;
        }
    }
    Ok(())
}

pub fn set_tray_visible(app: &AppHandle, visible: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_visible(visible) {
            log::warn!("tray icon: {e}");
        }
    }
}

/// Adds or removes the Windows start-up entry so that it matches the setting.
///
/// Skipped in a debug build: the entry would point at the dev executable, which has no frontend
/// without the dev server and would open a blank window at every sign-in.
pub fn sync_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    let autostart = app.autolaunch();
    if autostart.is_enabled().unwrap_or(false) == enabled {
        return Ok(());
    }
    if enabled { autostart.enable() } else { autostart.disable() }.map_err(|e| e.to_string())
}

pub fn show_companion(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("companion") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
