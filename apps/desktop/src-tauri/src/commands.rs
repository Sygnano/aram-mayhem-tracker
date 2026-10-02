//! Commands invoked from the frontends. Errors are returned as strings for display.

use std::sync::Arc;

use league_api::lcu::{ChatStatus, LcuClient};
use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::config::Settings;
use crate::engine::snapshot::AppSnapshot;
use crate::engine::Engine;
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
        let shell = (cfg.keep_in_tray, cfg.start_minimized);
        st.settings_dirty = true;
        shell
    };
    engine.save_settings_if_dirty();
    crate::shell::set_tray_visible(&app, keep_in_tray);
    crate::shell::sync_autostart(&app, start_minimized)
}

/// Swaps the local player's champion for one on the champ-select bench.
///
/// The only write the champ-select overlay performs, and it only ever happens because the user
/// clicked the badge for that champion. It is the same action as clicking the card in the client.
///
/// The champion is checked against the bench the LCU last reported rather than trusted from the
/// frontend: a stale overlay must not be able to ask for a swap that is no longer on offer.
#[tauri::command]
pub async fn swap_to_champion(engine: Shared<'_>, champion_id: i64) -> Result<(), String> {
    let (credentials, on_bench) = {
        let st = engine.lock();
        let on_bench = st.champ_select.session.as_ref().is_some_and(|s| s.is_on_bench(champion_id));
        (st.client.credentials.clone(), on_bench)
    };
    let credentials = credentials.ok_or("the League client was not found")?;
    if !on_bench {
        return Err(format!("champion {champion_id} is not on the bench any more"));
    }
    let result = LcuClient::new(&credentials).bench_swap(champion_id).await;
    let mut st = engine.lock();
    match result {
        Ok(()) => {
            st.champ_select.last_swap = Some(format!("swapped to {champion_id}"));
            st.log_event(format!("champ select: swapped to champion {champion_id}"));
            Ok(())
        }
        Err(e) => {
            let message = format!("swap failed: {e}");
            st.champ_select.last_swap = Some(message.clone());
            st.log_event(format!("champ select: {message}"));
            Err(message)
        }
    }
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
