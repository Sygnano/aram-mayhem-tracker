//! LCU loop: discovery, gameflow phase, queue, locale, static data.

use std::sync::Arc;
use std::time::{Duration, Instant};

use league_api::lcu::{GameflowPhase, LcuClient, QueueInfo};
use league_api::{lockfile, ApiError};
use static_data::{CDragon, StaticData};

use super::Engine;

const POLL: Duration = Duration::from_secs(1);
const IDLE_POLL: Duration = Duration::from_secs(3);
const SESSION_REFRESH: Duration = Duration::from_secs(5);
/// While in queue with auto accept on. The accept is timed from the ready check appearing, which a
/// 1 Hz poll would see up to a second late.
const QUEUE_POLL: Duration = Duration::from_millis(250);
/// How long a ready check is left alone before it is accepted.
const ACCEPT_DELAY: Duration = Duration::from_secs(1);
/// Phase reads in a row that may time out before the client is taken to be gone.
///
/// A timeout is not a closed client. The LCU stops answering for seconds at a time exactly when it
/// is busiest (entering champ select, starting the game), and giving up on the first one cleared
/// the locked champion and the whole champ-select state, blanking the overlay at the moment it was
/// wanted. Three is about ten seconds of silence at a 3 s timeout.
const TIMEOUTS_BEFORE_CLOSED: u32 = 3;

/// Does a failed read of the gameflow phase mean the client has gone?
///
/// Only a refused connection says so outright: nothing is listening on the port. A transport error
/// (a timeout, a reset) says so only once it has kept happening. A status or a body we could not
/// decode is an answer, so somebody is there.
fn client_is_gone(error: &ApiError, failures_in_a_row: u32) -> bool {
    match error {
        ApiError::NotRunning => true,
        ApiError::Transport(_) => failures_in_a_row >= TIMEOUTS_BEFORE_CLOSED,
        ApiError::Status { .. } | ApiError::Decode(_) => false,
    }
}

pub async fn run(engine: Arc<Engine>) {
    let mut lcu: Option<LcuClient> = None;
    let mut failures_in_a_row = 0u32;
    let mut last_session = Instant::now() - SESSION_REFRESH;
    // When the ready check on screen was first seen, and whether it still wants an answer from us.
    let mut ready_check: Option<(Instant, bool)> = None;

    loop {
        ensure_static_data(&engine, false);

        if lcu.is_none() {
            let configured = engine.lock().settings.league_dir.clone();
            let candidates = tokio::task::spawn_blocking(move || lockfile::candidates(configured.as_deref()))
                .await
                .unwrap_or_default();
            // A lockfile is only a claim that a client is listening. One left behind by a crash, or
            // written by a client that is still starting, has nobody behind it: try the next, and
            // failing that come back on the next idle poll.
            let mut found = None;
            for candidate in candidates {
                let client = LcuClient::new(&candidate.credentials);
                match client.region_locale().await {
                    Err(ApiError::NotRunning | ApiError::Transport(_)) => continue,
                    locale => {
                        found = Some((candidate, client, locale.ok().flatten()));
                        break;
                    }
                }
            }
            match found {
                Some((d, client, locale)) => {
                    let mut st = engine.lock();
                    st.client.connected = true;
                    st.client.credentials = Some(d.credentials.clone());
                    st.client.install_dir = d.install_dir.or_else(|| st.settings.league_dir.clone());
                    // Remember where the client was found, so the directory is known next time
                    // before the client is up. Never over one the user typed in.
                    if st.settings.league_dir.is_none() && st.client.install_dir.is_some() {
                        st.settings.league_dir = st.client.install_dir.clone();
                        st.settings_dirty = true;
                    }
                    st.client.error = None;
                    if locale.is_some() && st.client.locale != locale {
                        st.client.locale = locale;
                        // Names must be in the game's language: reload for the new locale.
                        st.static_data = None;
                        st.static_error = None;
                    }
                    st.log_event(format!("League client found on port {}", d.credentials.port));
                    drop(st);
                    engine.save_settings_if_dirty();
                    lcu = Some(client);
                    failures_in_a_row = 0;
                    last_session = Instant::now() - SESSION_REFRESH;
                }
                None => {
                    let mut st = engine.lock();
                    st.client.connected = false;
                    st.client.credentials = None;
                }
            }
        }

        if let Some(client) = &lcu {
            match client.gameflow_phase().await {
                Ok(phase) => {
                    failures_in_a_row = 0;
                    let changed = engine.lock().client.phase.as_ref() != Some(&phase);
                    if changed || last_session.elapsed() >= SESSION_REFRESH {
                        last_session = Instant::now();
                        let session = client.gameflow_session().await.ok();
                        let queue = session.as_ref().and_then(QueueInfo::from_session);
                        let mut st = engine.lock();
                        if changed {
                            st.log_event(format!("gameflow phase: {}", String::from(phase.clone())));
                        }
                        // Keep the last known queue through `None`/`Lobby` gaps, so an in-game
                        // snapshot still knows which queue it came from.
                        if queue.is_some() || matches!(phase, GameflowPhase::Lobby | GameflowPhase::Matchmaking) {
                            st.client.queue = queue;
                        }
                    }
                    if matches!(phase, GameflowPhase::ReadyCheck) {
                        let (seen, pending) = ready_check.get_or_insert((Instant::now(), true));
                        let wanted = engine.lock().settings.auto_accept;
                        if wanted && *pending && seen.elapsed() >= ACCEPT_DELAY {
                            *pending = !accept_ready_check(client, &engine).await;
                        }
                    } else {
                        ready_check = None;
                    }
                    // Only ask during champ select; elsewhere the endpoint 404s and the answer
                    // would be meaningless anyway. One read serves both the item-set writer (which
                    // wants our champion) and the champ-select overlay (which wants all of them).
                    let in_champ_select = matches!(phase, GameflowPhase::ChampSelect);
                    let champ_select = if in_champ_select { Some(client.champ_select().await) } else { None };

                    let mut st = engine.lock();
                    st.client.phase = Some(phase);
                    // The phase is already current, so this asks about now rather than the
                    // previous tick.
                    let aram = st.in_aram_champ_select();

                    match champ_select {
                        // Champ select, and the client answered: this is the truth.
                        Some(Ok(cs)) => {
                            st.client.locked_champion = cs.local_champion;
                            st.champ_select.session = Some(cs).filter(|_| aram);
                            st.champ_select.stale = false;
                        }
                        // Champ select, but the read failed. **Keep what we had.** A dropped read is
                        // not the end of champ select, and treating it as one made the overlay
                        // vanish — picking a champion churns the client's state, which is exactly
                        // when a read is most likely to time out or answer 404 for a moment.
                        Some(Err(e)) => {
                            if !st.champ_select.stale {
                                st.log_event(format!("champ select read failed, keeping the last one: {e}"));
                            }
                            st.champ_select.stale = true;
                        }
                        // Not in champ select any more: it really is over.
                        None => {
                            if st.client.locked_champion.is_some() {
                                // Forget what we wrote so the next lock-in writes again.
                                st.item_sets.written_for = None;
                            }
                            st.client.locked_champion = None;
                            st.champ_select.clear();
                        }
                    }
                    st.client.error = None;
                }
                Err(e) => {
                    failures_in_a_row += 1;
                    let mut st = engine.lock();
                    if client_is_gone(&e, failures_in_a_row) {
                        st.client.connected = false;
                        st.client.credentials = None;
                        st.client.locked_champion = None;
                        st.client.phase = None;
                        st.client.error = None;
                        st.champ_select.clear();
                        st.log_event(match e {
                            ApiError::NotRunning => "League client closed".to_owned(),
                            _ => format!("League client stopped answering ({e}); looking for it again"),
                        });
                        lcu = None;
                        ready_check = None;
                        failures_in_a_row = 0;
                    } else if matches!(e, ApiError::Transport(_)) {
                        // **Keep what we had**, the same rule as for a champ-select read: the phase,
                        // the locked champion and the blocks all stay as they were.
                        if failures_in_a_row == 1 {
                            st.log_event(format!("League client did not answer ({e}), keeping the last state"));
                        }
                    } else {
                        st.client.error = Some(e.to_string());
                    }
                }
            }
        }

        let in_queue = {
            let st = engine.lock();
            st.settings.auto_accept
                && matches!(st.client.phase, Some(GameflowPhase::Matchmaking | GameflowPhase::ReadyCheck))
        };
        let pause = match (&lcu, in_queue) {
            (None, _) => IDLE_POLL,
            (Some(_), true) => QUEUE_POLL,
            (Some(_), false) => POLL,
        };
        tokio::time::sleep(pause).await;
    }
}

/// Accepts the ready check on screen, unless the player has already answered it themselves.
///
/// Returns whether the ready check is dealt with. A failed read is not: it is asked again on the
/// next tick. A failed accept is, so one bad ready check is one line in the log and not forty.
async fn accept_ready_check(client: &LcuClient, engine: &Engine) -> bool {
    match client.ready_check_answered().await {
        Ok(true) => true,
        Ok(false) => {
            let result = client.accept_ready_check().await;
            engine.lock().log_event(match result {
                Ok(()) => "ready check accepted".into(),
                Err(e) => format!("ready check accept failed: {e}"),
            });
            true
        }
        Err(_) => false,
    }
}

/// Starts a background load when there is no static data for the current locale. After a
/// failure only `force` retries, so the 1 Hz loop does not hammer CommunityDragon.
pub fn ensure_static_data(engine: &Arc<Engine>, force: bool) {
    let locale = {
        let mut st = engine.lock();
        if st.static_data.is_some() || st.static_loading || (st.static_error.is_some() && !force) {
            return;
        }
        st.static_loading = true;
        st.client.locale.clone()
    };
    let engine = engine.clone();
    tauri::async_runtime::spawn(async move {
        let cd = CDragon::new(engine.cache_dir.clone());
        let result = cd.load(locale.as_deref()).await;
        let mut st = engine.lock();
        st.static_loading = false;
        // The locale can arrive while this load is in flight (the first tick starts one before the
        // client is found). Keeping it would pin the vocabulary to the wrong language for the whole
        // session; dropping it lets the next tick load the right one.
        if st.client.locale != locale {
            let msg = format!(
                "static data for {} discarded: the game's locale is now {}",
                locale.as_deref().unwrap_or("default"),
                st.client.locale.as_deref().unwrap_or("default"),
            );
            st.log_event(msg);
            return;
        }
        match result {
            Ok(data) => st.set_static_data(data),
            Err(e) => {
                st.static_error = Some(e.to_string());
                st.log_event(format!("static data failed: {e}"));
            }
        }
    });
}

/// Retry backoff after a failed static-data load, so the client loop does not hammer CDragon.
///
/// Also retries the champion index on its own. The static data loads without it, because offers can
/// still be read and named; but with no index no champion resolves to an id, and every offer shows
/// "champion unknown" until it comes back.
pub async fn static_retry_loop(engine: Arc<Engine>) {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let (failed, without_champions) = {
            let st = engine.lock();
            let failed = st.static_data.is_none() && !st.static_loading && st.static_error.is_some();
            let without = st.static_data.as_ref().filter(|d| d.champion_ids.is_empty()).map(|d| d.patch.clone());
            (failed, without)
        };
        if failed {
            ensure_static_data(&engine, true);
        }
        if let Some(patch) = without_champions {
            retry_champion_index(&engine, &patch).await;
        }
    }
}

async fn retry_champion_index(engine: &Arc<Engine>, patch: &str) {
    let result = CDragon::new(engine.cache_dir.clone()).champions(patch).await;
    let mut st = engine.lock();
    match result {
        Ok((ids, names)) => {
            // The static data may have been replaced while this ran (a locale change reloads it).
            // Only fill in the index it was fetched for, and only if it is still missing.
            let Some(current) = st.static_data.as_ref().filter(|d| d.patch == patch && d.champion_ids.is_empty())
            else {
                return;
            };
            let mut data = StaticData::clone(current);
            data.champion_ids = ids;
            data.champion_names = names;
            st.static_data = Some(Arc::new(data));
            st.log_event(format!("champion index for {patch} loaded on retry"));
        }
        // Debug only: offline this fails every minute, and the first failure is already logged.
        Err(e) => log::debug!("champion index for {patch} still unavailable: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_connection_is_a_closed_client_at_once() {
        assert!(client_is_gone(&ApiError::NotRunning, 1));
    }

    /// The case that used to blank the overlay: one timeout while the client was busy.
    #[test]
    fn a_timeout_only_counts_once_it_keeps_happening() {
        let timeout = ApiError::Transport("operation timed out".into());
        assert!(!client_is_gone(&timeout, 1));
        assert!(!client_is_gone(&timeout, TIMEOUTS_BEFORE_CLOSED - 1));
        assert!(client_is_gone(&timeout, TIMEOUTS_BEFORE_CLOSED));
    }

    #[test]
    fn an_answer_of_any_kind_means_somebody_is_there() {
        let status = ApiError::Status { status: 503, path: "/lol-gameflow/v1/gameflow-phase".into() };
        assert!(!client_is_gone(&status, 50));
        assert!(!client_is_gone(&ApiError::Decode("not a phase".into()), 50));
    }
}
