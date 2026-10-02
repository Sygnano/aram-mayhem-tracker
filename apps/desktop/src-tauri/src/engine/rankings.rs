//! Every champion's standing in ARAM: Mayhem, held in memory for champ select.
//!
//! **One fetch serves the whole screen.** A champ select shows up to eighteen champions at once —
//! three pick cards, ten bench slots, five allies — and the bench changes as it is rerolled, so
//! asking per champion would mean a burst of requests every few seconds and a blank block whenever
//! one was still in flight. `/v1/champions` answers with the entire table (about 15 KB), so this
//! fetches it once per patch and every lookup afterwards is a hash map hit.
//!
//! That is also why this is separate from [`super::champion`], which holds the augment pools and
//! build for the champion you *locked*: that data is per champion and worth fetching the moment a
//! lock happens, while this is one table for everyone and worth having before champ select opens at
//! all.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aramkit_client::{AramkitClient, ChampionRanking, Freshness};

use super::Engine;

/// How often to look for a new build. The service polls aramkit every six hours, so anything more
/// frequent only re-asks a question whose answer cannot have changed.
const REFRESH: Duration = Duration::from_secs(6 * 60 * 60);

/// How soon to try again while we have nothing at all. Short, because champ select can open at any
/// moment and a block with no numbers is the one thing this is meant to prevent.
const RETRY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default)]
pub struct Rankings {
    /// Keyed by Riot champion id.
    pub by_id: HashMap<i64, ChampionRanking>,
    /// What a rank is out of. Never assumed: it comes from the service alongside the ranks, because
    /// the number of ranked champions moves with each patch and "#3 / 173" is a different claim
    /// from "#3 / 168".
    pub pool_size: i64,
    pub patch: String,
    pub data_date: String,
    /// `Stale` when this came off the on-disk copy because the service was unreachable.
    pub freshness: Option<Freshness>,
    pub error: Option<String>,
}

impl Rankings {
    pub fn get(&self, champion_id: i64) -> Option<&ChampionRanking> {
        self.by_id.get(&champion_id)
    }

    /// We have a table to look champions up in.
    pub fn is_loaded(&self) -> bool {
        !self.by_id.is_empty() && self.pool_size > 0
    }
}

pub fn spawn(engine: Arc<Engine>) {
    super::spawn_task(&engine, "the champion table", run(engine.clone()));
}

async fn run(engine: Arc<Engine>) {
    let (base, cache_dir) = {
        let state = engine.lock();
        (state.tuning.service_base.clone(), engine.cache_dir.join("aramkit"))
    };
    let client = AramkitClient::new(base, cache_dir);

    loop {
        let loaded = match client.champions().await {
            Ok(fetched) => {
                let response = fetched.value;
                let by_id: HashMap<i64, ChampionRanking> = response.champions.into_iter().map(|c| (c.id, c)).collect();
                let count = by_id.len();
                let pool_size = response.pool_size;

                let mut st = engine.lock();
                st.rankings = Rankings {
                    by_id,
                    pool_size,
                    patch: response.patch,
                    data_date: response.data_date,
                    freshness: Some(fetched.freshness),
                    error: None,
                };
                let loaded = st.rankings.is_loaded();
                let note = format!(
                    "champion rankings ready: {count} champions of {pool_size} on {} ({})",
                    st.rankings.patch,
                    match fetched.freshness {
                        Freshness::Live => "live",
                        Freshness::Stale => "offline copy",
                    }
                );
                st.log_event(note);
                loaded
            }
            Err(e) => {
                // The previous table, if there is one, is kept: last patch's ranks are far closer to
                // the truth than no ranks, and the error is surfaced rather than swallowed.
                let mut st = engine.lock();
                st.rankings.error = Some(e.to_string());
                let loaded = st.rankings.is_loaded();
                st.log_event(format!("could not fetch champion rankings: {e}"));
                loaded
            }
        };

        tokio::time::sleep(if loaded { REFRESH } else { RETRY }).await;
    }
}
