//! Commands invoked from the frontends. Errors are returned as strings for display.

use std::sync::Arc;

use league_api::lcu::{ChatStatus, LcuClient};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::config::Settings;
use crate::engine::champselect::{self, ClickDecision};
use crate::engine::snapshot::AppSnapshot;
use crate::engine::Engine;
use crate::portable::Mode;
use crate::updates::{UpdateStatus, Updates};

type Shared<'a> = State<'a, Arc<Engine>>;

#[tauri::command]
pub fn get_snapshot(engine: Shared<'_>) -> AppSnapshot {
    engine.lock().snapshot()
}

/// The settings the companion window edits. Named for the file they are kept in.
#[tauri::command]
pub fn get_config(engine: Shared<'_>) -> Settings {
    engine.lock().settings.clone()
}

/// A change to the settings. Every field is optional and an absent one is left as it is, so a
/// checkbox can save itself without knowing the rest.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    /// An empty string clears it, which hands the choice back to discovery.
    league_dir: Option<String>,
    overlay_enabled: Option<bool>,
    simple_mode: Option<bool>,
    show_augment_list: Option<bool>,
    manage_item_sets: Option<bool>,
    auto_accept: Option<bool>,
    keep_in_tray: Option<bool>,
    start_minimized: Option<bool>,
    check_updates_on_startup: Option<bool>,
    auto_update_statistics: Option<bool>,
}

#[tauri::command]
pub fn save_settings(app: AppHandle, engine: Shared<'_>, settings: SettingsPatch) -> Result<(), String> {
    let (keep_in_tray, start_minimized) = {
        let mut st = engine.lock();
        let cfg = &mut st.settings;
        if let Some(dir) = settings.league_dir {
            let dir = dir.trim();
            cfg.league_dir = (!dir.is_empty()).then(|| dir.into());
        }
        if let Some(on) = settings.overlay_enabled {
            cfg.overlay_enabled = on;
        }
        if let Some(on) = settings.simple_mode {
            cfg.simple_mode = on;
        }
        if let Some(on) = settings.show_augment_list {
            cfg.show_augment_list = on;
        }
        if let Some(on) = settings.manage_item_sets {
            cfg.manage_item_sets = on;
        }
        if let Some(on) = settings.auto_accept {
            cfg.auto_accept = on;
        }
        if let Some(on) = settings.keep_in_tray {
            cfg.keep_in_tray = on;
        }
        if let Some(on) = settings.start_minimized {
            cfg.start_minimized = on;
        }
        if let Some(on) = settings.check_updates_on_startup {
            cfg.check_updates_on_startup = on;
        }
        if let Some(on) = settings.auto_update_statistics {
            cfg.auto_update_statistics = on;
        }
        let shell = (cfg.keep_in_tray, cfg.start_minimized);
        st.settings_dirty = true;
        shell
    };
    engine.save_settings_if_dirty();
    crate::shell::set_tray_visible(&app, keep_in_tray);
    crate::shell::sync_autostart(&app, start_minimized)
}

/// Looks for newer statistics now, whatever the automatic update says. The download runs in the
/// background; the snapshot follows it.
#[tauri::command]
pub fn update_statistics(engine: Shared<'_>) {
    crate::engine::dataset::request_update(&mut engine.lock());
}

/// A click on a champ-select bench block: swaps to that champion, or arms the swap for later.
///
/// The champ-select overlay's only write, and it always starts with the user clicking the block
/// for that champion. Before swaps open (D-094), a teammate's champion cannot be swapped to yet, so
/// the click arms it and the champ-select loop sends the swap the moment they open. A second click
/// on the same champion cancels it. A champion from the player's own pool is swapped to at once.
///
/// The champion is checked against the bench the LCU last reported rather than trusted from the
/// frontend: a stale overlay must not be able to ask for a swap that is no longer on offer.
#[tauri::command]
pub async fn swap_to_champion(engine: Shared<'_>, champion_id: i64) -> Result<ClickDecision, String> {
    let decision = {
        let mut st = engine.lock();
        let decision = st.champ_select.click(champion_id)?;
        match decision {
            ClickDecision::Armed => st.log_event(format!("champ select: armed a swap to champion {champion_id}")),
            ClickDecision::Disarmed => st.log_event(format!("champ select: cancelled the swap to {champion_id}")),
            ClickDecision::SwapNow => {}
        }
        decision
    };
    if decision == ClickDecision::SwapNow {
        champselect::send_swap(engine.inner(), champion_id, "click").await?;
    }
    Ok(decision)
}

/// The chat status the client reports now, for the companion window's radio buttons. `None` when
/// the client is closed, the read fails, or the status is one the buttons do not offer.
#[tauri::command]
pub async fn get_chat_status(engine: Shared<'_>) -> Result<Option<ChatStatus>, String> {
    let Some(credentials) = engine.lock().client.credentials.clone() else {
        return Ok(None);
    };
    Ok(LcuClient::new(&credentials).chat_status().await.ok().flatten())
}

/// Sets the chat status. Sent once, when the user picks a radio button; nothing re-applies
/// it if the client changes the status afterwards.
#[tauri::command]
pub async fn set_chat_status(engine: Shared<'_>, status: ChatStatus) -> Result<(), String> {
    let credentials = engine.lock().client.credentials.clone().ok_or("the League client was not found")?;
    let result = LcuClient::new(&credentials).set_chat_status(status).await;
    let mut st = engine.lock();
    match result {
        Ok(()) => {
            st.log_event(format!("chat status set to {}", status.availability()));
            Ok(())
        }
        Err(e) => {
            let message = format!("chat status not set: {e}");
            st.log_event(message.clone());
            Err(message)
        }
    }
}

/// A render error caught by a window's error boundary, sent here because the webview has no log
/// of its own: without this a broken overlay is simply blank, with nothing anywhere to say why.
#[tauri::command]
pub fn report_frontend_error(engine: Shared<'_>, window: tauri::Window, message: String, stack: Option<String>) {
    // Bounded: this is text from the webview, and it goes to a file.
    let clip = |text: &str, max: usize| text.chars().take(max).collect::<String>();
    log::error!(
        "{} window: {}\n{}",
        window.label(),
        clip(&message, 500),
        clip(stack.as_deref().unwrap_or("no stack"), 4000)
    );
    engine.lock().log_event(format!("the {} window hit an error: {}", window.label(), clip(&message, 200)));
}

/// Where the update check stands: the companion window asks while a check or a download runs.
#[tauri::command]
pub fn get_update_status(updates: State<'_, Arc<Updates>>) -> UpdateStatus {
    updates.status()
}

/// Asks GitHub Releases for a newer version.
#[tauri::command]
pub async fn check_for_updates(app: AppHandle, updates: State<'_, Arc<Updates>>) -> Result<UpdateStatus, String> {
    Ok(updates.check(&app).await)
}

/// Downloads and installs the update the last check found. The installer closes the app, so this
/// only returns when something went wrong.
#[tauri::command]
pub async fn install_update(updates: State<'_, Arc<Updates>>) -> Result<(), String> {
    updates.install().await
}

/// How this copy runs, for the companion window: a portable copy gets the release page instead of
/// the installer, and a note on where its settings are.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallMode {
    portable: bool,
    /// The `data` folder beside the executable; `None` when installed, or when that folder could not
    /// be written and the installed copy's folders are used instead.
    data_dir: Option<String>,
}

#[tauri::command]
pub fn get_install_mode(mode: State<'_, Mode>) -> InstallMode {
    InstallMode { portable: mode.is_portable(), data_dir: mode.data_dir().map(|d| d.display().to_string()) }
}

/// Opens the latest release on GitHub, where a portable copy finds its new zip.
#[tauri::command]
pub fn open_releases_page() -> Result<(), String> {
    crate::portable::open_releases_page()
}
