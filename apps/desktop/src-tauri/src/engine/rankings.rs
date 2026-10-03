//! Every champion's standing in ARAM: Mayhem, held in memory for champ select.
//!
//! **One table serves the whole screen.** A champ select shows up to eighteen champions at once —
//! three pick cards, ten bench slots, five allies — and the bench changes as it is rerolled, so every
//! lookup is a hash map hit on a table held in memory. The table comes with the downloaded dataset
//! ([`super::dataset`], D-090), so it is there before champ select opens and never fetched on its
//! own.
//!
//! It is separate from [`super::champion`], which holds the augment pools and build for the
//! champion you *locked*, read from disk at the moment of the lock.

use std::collections::HashMap;

use aramkit_client::{ChampionRanking, ChampionsResponse, Freshness};

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
    /// `Stale` when this came off an on-disk copy because the service was unreachable. `None` for
    /// the downloaded dataset, which is on disk by design.
    pub freshness: Option<Freshness>,
    pub error: Option<String>,
}

impl Rankings {
    /// The table as the dataset holds it.
    pub fn from_table(table: &ChampionsResponse) -> Self {
        Self {
            by_id: table.champions.iter().map(|c| (c.id, c.clone())).collect(),
            pool_size: table.pool_size,
            patch: table.patch.clone(),
            data_date: table.data_date.clone(),
            freshness: None,
            error: None,
        }
    }

    pub fn get(&self, champion_id: i64) -> Option<&ChampionRanking> {
        self.by_id.get(&champion_id)
    }
}
