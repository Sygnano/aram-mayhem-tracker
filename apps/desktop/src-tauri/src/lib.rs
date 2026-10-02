//! ARAM Mayhem Tracker desktop app: wires the engine to two windows.

mod commands;
mod config;
mod engine;
mod logging;
mod overlay;
mod shell;
mod updates;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};

use engine::{Engine, OverlayTarget};
use overlay::OverlayState;

const PUBLISH_EVERY: Duration = Duration::from_millis(250);
/// How often the overlay is re-placed over its host and the pointer is checked against its buttons.
///
/// Roughly a display frame. Placement used to ride along with the snapshot at 4 Hz, which is fine
/// while a window sits still and visibly wrong the moment it moves: dragging the client left the
/// overlay lurching a quarter of a second behind it. Both jobs here are cheap — find the window, read
/// its rectangle, and `SetWindowPos` only when it actually changed — so the rate is set by how smooth
/// it should look rather than by what it costs.
const PLACE_EVERY: Duration = Duration::from_millis(16);
/// The same loop while the overlay has no host and is hidden: there is nothing to keep glued, only
/// a host to wait for, and a quarter of a second is how late the overlay may then appear.
const PLACE_EVERY_IDLE: Duration = Duration::from_millis(250);
/// While an augment offer is on screen the snapshot is published faster, so a reroll reaches the
/// panels in about a tenth of a second instead of a quarter. It only applies for the few seconds
/// cards are up, which is the only moment the extra traffic buys anything.
const PUBLISH_EVERY_WITH_CARDS: Duration = Duration::from_millis(100);
/// An unchanged snapshot is still sent this often. The windows fetch one when they load, so this is
/// only a floor under anything that could have missed an event.
const PUBLISH_AT_LEAST_EVERY: Duration = Duration::from_secs(2);
/// How long after start the update check waits, when the setting asks for one.
const UPDATE_CHECK_DELAY: Duration = Duration::from_secs(5);

pub fn run() {
    tauri::Builder::default()
        // First, so a second launch exits before it sets anything up. Two copies would mean two
        // engines: two overlays, two screen readers, item sets written twice and a ready check
        // accepted twice. Launching the app again while it runs (from the Start menu, say, with the
        // start-up copy in the tray) brings up the running copy's window instead.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|a| a == shell::MINIMIZED_ARG) {
                shell::show_companion(app);
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![shell::MINIMIZED_ARG]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_config,
            commands::save_settings,
            commands::swap_to_champion,
            commands::get_chat_status,
            commands::set_chat_status,
            commands::report_frontend_error,
            commands::get_update_status,
            commands::check_for_updates,
            commands::install_update,
        ])
        .setup(|app| {
            let paths = app.path();
            // First, so that everything after it, the engine's own start included, is on record.
            let log_file = paths.app_log_dir().ok().and_then(|dir| logging::init(&dir));
            let config_path = paths.app_config_dir()?.join("config.json");
            let cache_dir = paths.app_cache_dir()?.join("cdragon");
            let engine = Arc::new(Engine::new(config_path, cache_dir));
            engine.lock().log_file = log_file;
            app.manage(engine.clone());
            let updates = Arc::new(updates::Updates::default());
            app.manage(updates.clone());

            engine::spawn_task(&engine, "the League client connection", engine::client::run(engine.clone()));
            engine::spawn_task(&engine, "the game data retry", engine::client::static_retry_loop(engine.clone()));
            engine::spawn_task(&engine, "the live game reader", engine::game::run(engine.clone()));
            engine::vision::spawn(engine.clone());
            engine::champion::spawn(engine.clone());
            engine::rankings::spawn(engine.clone());
            engine::anvils::spawn(engine.clone());
            engine::champselect::spawn(engine.clone());
            engine::stats::spawn(engine.clone());
            engine::itemsets::spawn(engine.clone());

            let handle = app.handle().clone();
            // The setting is read once, at start: that is the only moment it is about.
            if engine.lock().settings.check_updates_on_startup {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    // After the client connection and the game data, which matter more at start.
                    tokio::time::sleep(UPDATE_CHECK_DELAY).await;
                    updates.check(&handle).await;
                });
            }
            shell::setup(&handle, &engine)?;
            overlay::setup(&handle);

            // Placement and pointer tracking, at frame rate. Kept apart from the snapshot because
            // they follow the *window*, which moves whenever the user drags it, while the snapshot
            // follows the *game*, which changes far more slowly and costs much more to build.
            engine::spawn_task(&engine, "the overlay placement", {
                let handle = handle.clone();
                let engine = engine.clone();
                async move {
                    // Owned by this task alone: nothing else places the overlay.
                    let mut overlay_state = OverlayState::default();
                    loop {
                        let (target, buttons) = {
                            let st = engine.lock();
                            (st.overlay_target(), st.champ_select.clickable_screen_rects())
                        };
                        let status = overlay::sync(&handle, &mut overlay_state, target);
                        overlay::sync_click_through(&handle, &mut overlay_state, &buttons);
                        engine.lock().overlay_status = status;
                        let idle = target == OverlayTarget::None && !status.visible;
                        tokio::time::sleep(if idle { PLACE_EVERY_IDLE } else { PLACE_EVERY }).await;
                    }
                }
            });

            engine::spawn_task(&engine, "the snapshot publisher", {
                let engine = engine.clone();
                async move {
                    // The last payload sent, and when. With nothing running the snapshot does not
                    // change from one tick to the next, and sending it again would only make both
                    // webviews parse and re-render the same thing four times a second.
                    let mut sent: Option<(String, Instant)> = None;
                    loop {
                        let (snapshot, cards_up) = {
                            let st = engine.lock();
                            (st.snapshot(), st.vision.cards_on_screen)
                        };
                        match serde_json::to_string(&snapshot) {
                            Ok(json) => {
                                let unchanged = sent
                                    .as_ref()
                                    .is_some_and(|(last, at)| *last == json && at.elapsed() < PUBLISH_AT_LEAST_EVERY);
                                if !unchanged {
                                    // Emitted as the JSON already built, so it is serialised once.
                                    match serde_json::value::RawValue::from_string(json.clone()) {
                                        Ok(raw) => {
                                            if let Err(e) = handle.emit("snapshot", raw) {
                                                log::warn!("emit snapshot: {e}");
                                            }
                                        }
                                        Err(e) => log::warn!("snapshot is not valid JSON: {e}"),
                                    }
                                    sent = Some((json, Instant::now()));
                                }
                            }
                            Err(e) => log::warn!("serialise snapshot: {e}"),
                        }
                        tokio::time::sleep(if cards_up { PUBLISH_EVERY_WITH_CARDS } else { PUBLISH_EVERY }).await;
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the companion quits, unless it is kept in the tray; the overlay only hides.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let to_tray = app.state::<Arc<Engine>>().lock().settings.keep_in_tray;
                if window.label() != "companion" || to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    app.exit(0);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
