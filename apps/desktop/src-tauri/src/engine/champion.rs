//! Everything about the champion you locked, read from the downloaded dataset (D-090).
//!
//! The augment offer lasts seconds and the reroll button is instant, so nothing in game may wait on
//! a network call, and nothing does: every champion's numbers are on disk before the overlay shows
//! anything (`dataset.rs`). A lock reads one file, about 650 KB, with the ranked augment pool for
//! each rarity at each of the four stages and the build archetypes the item sets are made from.
//!
//! In ARAM nothing is locked by hand: the client assigns a champion when champ select opens, and
//! every bench swap changes it. Each change reads that champion's file again.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aramkit_client::{BuildResponse, Freshness, PoolResponse, StoredDataset};
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
    /// `Stale` when this came off an on-disk copy because the service was unreachable. `None` for
    /// the downloaded dataset, which is on disk by design.
    pub freshness: Option<Freshness>,
    /// The dataset folder this was read from. A newer dataset is read again, and until it has been,
    /// this stays in use: an update landing mid-offer must not blank the cards.
    pub dataset_folder: Option<String>,
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
    loop {
        tokio::time::sleep(TICK).await;

        let (champion_id, dataset) = {
            let st = engine.lock();
            let Some(champion_id) = wanted_champion(&st) else { continue };
            // Nothing is shown before the dataset is on disk, so there is nothing to read yet.
            let Some(dataset) = st.dataset.current.clone() else { continue };
            // Held already, from this dataset.
            if st.champion.champion_id == Some(champion_id)
                && st.champion.dataset_folder.as_deref() == Some(dataset.manifest.folder.as_str())
            {
                continue;
            }
            (champion_id, dataset)
        };

        let read = tokio::task::spawn_blocking({
            let dataset = dataset.clone();
            move || dataset.champion(champion_id)
        })
        .await;

        let mut st = engine.lock();
        // The champion or the dataset changed while this read: the next tick reads again.
        let current = st.dataset.current.as_ref().is_some_and(|d| Arc::ptr_eq(d, &dataset));
        if wanted_champion(&st) != Some(champion_id) || !current {
            continue;
        }
        st.champion = match read {
            Ok(Ok(Some(bundle))) => {
                let mut data = from_bundle(champion_id, &bundle);
                data.dataset_folder = Some(dataset.manifest.folder.clone());
                let note = format!(
                    "champion {champion_id} ready: {} pools and the build, from the statistics of {}",
                    data.pools.len(),
                    bundle.data_date
                );
                st.log_event(note);
                data
            }
            Ok(Ok(None)) => {
                let error = format!("no statistics for champion {champion_id} in this dataset");
                st.log_event(error.clone());
                ChampionData {
                    champion_id: Some(champion_id),
                    error: Some(error),
                    dataset_folder: Some(dataset.manifest.folder.clone()),
                    ..Default::default()
                }
            }
            Ok(Err(e)) => failed(&mut st, champion_id, &dataset, e.to_string()),
            Err(e) => failed(&mut st, champion_id, &dataset, e.to_string()),
        };
    }
}

/// The champion's data as the stats engine and the item-set writer read it.
fn from_bundle(champion_id: i64, bundle: &aramkit_client::ChampionBundle) -> ChampionData {
    let pools: HashMap<(Rarity, u8), PoolResponse> =
        bundle.pool_responses().filter_map(|p| Some(((Rarity::parse(&p.rarity)?, p.stage?), p))).collect();
    let expected = RARITIES.len() * STAGES.len();
    let error = (pools.len() < expected).then(|| format!("the dataset holds {} of {expected} pools", pools.len()));
    ChampionData {
        champion_id: Some(champion_id),
        pools,
        build: Some(bundle.build()),
        loading: false,
        error,
        freshness: None,
        dataset_folder: None,
    }
}

fn failed(st: &mut super::EngineState, champion_id: i64, dataset: &StoredDataset, error: String) -> ChampionData {
    st.log_event(format!(
        "champion {champion_id}: the statistics in {} could not be read: {error}",
        dataset.dir.display()
    ));
    ChampionData {
        champion_id: Some(champion_id),
        error: Some(error),
        dataset_folder: Some(dataset.manifest.folder.clone()),
        ..Default::default()
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
