//! Live Client Data loop: owns the game session.

use std::sync::Arc;
use std::time::Duration;

use league_api::lcd::{AllGameData, LiveClient};
use league_api::ApiError;
use mayhem_core::augments::OfferTracker;
use mayhem_core::clock::GameClock;

use super::{Engine, EngineState, GameSession};

const IN_GAME_POLL: Duration = Duration::from_millis(250);
const IDLE_POLL: Duration = Duration::from_secs(1);
/// A game clock that goes back by more than this is a new game.
const NEW_GAME_REWIND: f64 = 5.0;

pub async fn run(engine: Arc<Engine>) {
    let lcd = LiveClient::default();
    loop {
        let result = lcd.all_game_data().await;
        let in_game = {
            let mut st = engine.lock();
            let now = engine.now();
            match result {
                Ok(data) => {
                    apply(&mut st, now, data);
                    true
                }
                // Refused: no game process. 404: loading screen, no data yet. Either way, no game.
                Err(ApiError::NotRunning) | Err(ApiError::Status { status: 404, .. }) => {
                    end_session(&mut st);
                    false
                }
                Err(e) => {
                    // Transient (timeout mid-game, a malformed payload): keep the session.
                    log::debug!("live client error: {e}");
                    st.game.is_some()
                }
            }
        };
        if !in_game {
            engine.save_settings_if_dirty();
        }
        tokio::time::sleep(if in_game { IN_GAME_POLL } else { IDLE_POLL }).await;
    }
}

fn apply(st: &mut EngineState, now: f64, data: AllGameData) {
    let game_time = data.game_data.game_time;
    let rewound = st.game.as_ref().is_some_and(|g| game_time + NEW_GAME_REWIND < g.last_game_time);
    if rewound {
        end_session(st);
    }
    if st.game.is_none() {
        start_session(st, &data);
    }
    let is_mayhem = st.is_mayhem(&data);
    let game = st.game.as_mut().expect("session exists");
    game.clock.observe(now, game_time);
    game.last_game_time = game_time;
    game.is_mayhem = is_mayhem;
    game.data = data;
}

fn start_session(st: &mut EngineState, data: &AllGameData) {
    st.next_session_id += 1;
    let session = GameSession {
        id: st.next_session_id,
        clock: GameClock::default(),
        last_game_time: data.game_data.game_time,
        data: data.clone(),
        is_mayhem: st.is_mayhem(data),
        offers: OfferTracker::new(st.tuning.offer_tracker),
    };
    st.log_event(format!(
        "game started: mode {} map {} (mayhem: {})",
        data.game_data.game_mode, data.game_data.map_number, session.is_mayhem
    ));
    st.game = Some(session);
}

fn end_session(st: &mut EngineState) {
    if let Some(game) = st.game.take() {
        st.log_event(format!("game ended at {:.0}s; offers seen {}", game.last_game_time, game.offers.stages_seen()));
    }
}
