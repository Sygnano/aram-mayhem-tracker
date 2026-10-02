//! Everything about the champion you locked, fetched while champ select and the loading screen run.
//!
//! The augment offer lasts seconds and the reroll button is instant, so nothing in game should wait
//! on a network call. Locking a champion is the natural moment to pay that cost: there is a champ
//! select and a loading screen to spend, and by the time the first cards appear every answer is
//! already in memory.
//!
//! In ARAM nothing is locked by hand: the client assigns a champion when champ select opens, and
//! every bench swap changes it. Each change is one request, `/v1/champion`, which answers
//! with the ranked augment pool for each rarity at each of the four stages, the build archetypes the
//! item sets are made from, and the champion's anvil ranking group. It used to be fourteen requests
//! for the same data. A swap while the request is in flight drops it and asks for the new champion.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aramkit_client::{AramkitClient, BuildResponse, Freshness, PoolResponse};
use mayhem_core::ranking::Rarity;

use super::Engine;

/// How often to check whether the locked champion has changed.
const TICK: Duration = Duration::from_millis(500);

/// The rarities an offer can be, lowest first. The cards in an offer share one, except a
/// golden reroll, which is one step further along this list.
pub const RARITIES: [Rarity; 3] = Rarity::ALL;

/// The four offers, at levels 1, 7, 11 and 15.
pub const STAGES: [u8; 4] = [1, 2, 3, 4];

#[derive(Debug, Clone, Default)]
pub struct ChampionData {
    /// Who this data is for. `None` before the first lock-in.
    pub champion_id: Option<i64>,
    /// Ranked pools by `(rarity, stage)`.
    pub pools: HashMap<(Rarity, u8), PoolResponse>,
    pub build: Option<BuildResponse>,
    pub loading: bool,
    pub error: Option<String>,
    /// `Stale` when this came off the on-disk copy because the service was unreachable.
    pub freshness: Option<Freshness>,
}

impl ChampionData {
    pub fn pool(&self, champion_id: i64, rarity: Rarity, stage: u8) -> Option<&PoolResponse> {
        if self.champion_id != Some(champion_id) {
            return None;
        }
        self.pools.get(&(rarity, stage))
    }

    pub fn build_for(&self, champion_id: i64) -> Option<&BuildResponse> {
        if self.champion_id != Some(champion_id) {
            return None;
        }
        self.build.as_ref()
    }

    /// Everything arrived. Used by the UI to say whether the next offer will be instant.
    pub fn is_ready(&self, champion_id: i64) -> bool {
        self.champion_id == Some(champion_id)
            && self.build.is_some()
            && self.pools.len() == RARITIES.len() * STAGES.len()
    }
}

pub fn spawn(engine: Arc<Engine>) {
    super::spawn_task(&engine, "the champion data prefetch", run(engine.clone()));
}

async fn run(engine: Arc<Engine>) {
    let (base, cache_dir) = {
        let state = engine.lock();
        (state.tuning.service_base.clone(), engine.cache_dir.join("aramkit"))
    };
    let client = AramkitClient::new(base, cache_dir);

    loop {
        tokio::time::sleep(TICK).await;

        let Some(champion_id) = ({
            let st = engine.lock();
            wanted_champion(&st)
        }) else {
            continue;
        };

        {
            let mut st = engine.lock();
            if st.champion.champion_id == Some(champion_id) || st.champion.loading {
                continue;
            }
            // A new champion: drop the old data rather than answering with someone else's numbers.
            st.champion = ChampionData { champion_id: None, loading: true, ..Default::default() };
            st.log_event(format!("fetching augment, build and anvil data for champion {champion_id}"));
        }

        let mut still_wanted = tokio::time::interval(TICK);
        let fetched = tokio::select! {
            fetched = client.champion(champion_id) => Some(fetched),
            // The player swapped while this was in flight: the answer is for a champion they no
            // longer have, so stop waiting for it.
            _ = async {
                loop {
                    still_wanted.tick().await;
                    if wanted_champion(&engine.lock()) != Some(champion_id) {
                        break;
                    }
                }
            } => None,
        };

        let mut st = engine.lock();
        // Checked again under the lock that publishes the result; the next tick fetches for whoever
        // is wanted now.
        let Some(fetched) = fetched.filter(|_| wanted_champion(&st) == Some(champion_id)) else {
            st.champion = ChampionData::default();
            continue;
        };
        match fetched {
            Ok(bundle) => {
                let pools: HashMap<(Rarity, u8), PoolResponse> = bundle
                    .value
                    .pool_responses()
                    .filter_map(|p| Some(((Rarity::parse(&p.rarity)?, p.stage?), p)))
                    .collect();
                let count = pools.len();
                let expected = RARITIES.len() * STAGES.len();
                let error = (count < expected).then(|| format!("the service sent {count} of {expected} pools"));
                st.champion = ChampionData {
                    champion_id: Some(champion_id),
                    pools,
                    build: Some(bundle.value.build()),
                    loading: false,
                    error,
                    freshness: Some(bundle.freshness),
                };
                // The rankings come with the bundle, so an edit saved in `/admin` during one champ
                // select is live from the next champion on.
                let group = bundle.value.anvil_rankings.group_of(champion_id).map(|g| g.name.clone());
                st.anvils.rankings = Some(bundle.value.anvil_rankings);
                st.anvils.rankings_error = None;
                let anvils = match group {
                    Some(g) => format!("anvil group \"{g}\""),
                    None => "no anvil group".to_owned(),
                };
                let stale = if bundle.freshness == Freshness::Stale { ", from the stored copy" } else { "" };
                st.log_event(format!("champion {champion_id} ready: {count} pools, the build, {anvils}{stale}"));
            }
            Err(e) => {
                st.champion =
                    ChampionData { champion_id: Some(champion_id), error: Some(e.to_string()), ..Default::default() };
                st.anvils.rankings_error = Some(format!("anvil rankings: {e}"));
                st.log_event(format!("champion {champion_id} data not fetched: {e}"));
            }
        }
    }
}

/// The champion we should be holding data for.
///
/// Champ select is the point of this, but a game already in progress is used as a fallback so the
/// app still works when it was started mid-game and never saw a lock-in.
pub(crate) fn wanted_champion(st: &super::EngineState) -> Option<i64> {
    if let Some(id) = st.client.locked_champion {
        return Some(id);
    }
    let game = st.game.as_ref().filter(|g| g.is_mayhem)?;
    let key = game.data.me()?.champion_key();
    st.static_data.as_ref()?.champion_id(key)
}
