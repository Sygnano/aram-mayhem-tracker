//! Augment statistics for whatever is on screen.
//!
//! Everything for the locked champion is fetched during champ select (`champion.rs`), so this no
//! longer touches the network: it looks up the prefetched pool for the offer's rarity and stage and
//! picks out the three cards. That is what makes a reroll render as fast as OCR can name the card.
//!
//! One card can be a tier above the others: the golden reroll replaces a card with an augment one
//! rarity higher. The offer's rarity is therefore the *lowest* on screen, and a card from
//! the tier above is looked up in that tier's pool instead.
//!
//! The ranking falls out for free. The pool arrives sorted by the service's own rule — baseline tier
//! first, then delta — so the order the cards appear in it *is* their ranking, with no need
//! to reimplement that ordering here and risk it drifting from the server's. The one exception is
//! an offer holding an upgraded card, whose cards come from two pools and have to be merged.
//!
//! Everything the overlay shows about a row is decided here too: its block, its place in that
//! block and its grade ([`mayhem_core::ranking`]). The overlay draws what it is given.

use std::sync::Arc;
use std::time::Duration;

use aramkit_client::PoolResponse;
use mayhem_core::ranking::{ranks_within_blocks, Block, Grade, Rarity};

use super::snapshot::{OfferStats, PoolStats, RankedAugment};
use super::{Engine, EngineState};

/// How often to look at the screen. Nothing here blocks or waits on a network call, so this is just
/// the lag between a card being read and its numbers appearing.
const TICK: Duration = Duration::from_millis(100);

/// Identifies one question, so the same offer is never recomputed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsKey {
    pub champion_id: i64,
    pub stage: u8,
    /// Card ids in slot order. A reroll changes one and re-answers.
    pub augments: Vec<i64>,
    /// The offer's own rarity: the lowest among the cards, since a golden reroll only ever raises
    /// one. `None` when no card on screen has a rarity an offer can be, and then there is no pool
    /// to open.
    pub rarity: Option<Rarity>,
}

#[derive(Debug, Clone, Default)]
pub struct StatsState {
    /// What the current payload answers. `None` when there is nothing on screen.
    pub key: Option<StatsKey>,
    pub offer: Option<OfferStats>,
    /// The ranked pool for the offer's rarity, which the overlay opens top right.
    pub pool: Option<PoolStats>,
    /// The pool one tier above, held only while a card on screen comes from it (a golden reroll).
    pub upgraded_pool: Option<PoolStats>,
    pub loading: bool,
    pub error: Option<String>,
    /// An offer is on screen but the champion could not be identified, so no lookup is possible.
    pub champion_unknown: bool,
}

pub fn spawn(engine: Arc<Engine>) {
    super::spawn_task(&engine, "the offer statistics", run(engine.clone()));
}

async fn run(engine: Arc<Engine>) {
    loop {
        tokio::time::sleep(TICK).await;
        let mut state = engine.lock();
        update(&mut state);
    }
}

/// Recomputes the statistics view from what is on screen and what was prefetched.
fn update(state: &mut EngineState) {
    let Some(key) = current_key(state) else {
        // Nothing on screen: drop what we were showing, so a panel cannot outlive its offer.
        if state.stats.key.is_some() || state.stats.error.is_some() {
            let champion_unknown = state.stats.champion_unknown;
            state.stats = StatsState { champion_unknown, ..Default::default() };
        }
        return;
    };
    if state.stats.key.as_ref() == Some(&key) {
        return;
    }

    let found = key.rarity.and_then(|rarity| Some((rarity, state.champion.pool(key.champion_id, rarity, key.stage)?)));
    let Some((rarity, pool)) = found else {
        // The champ select prefetch has not landed, or it failed. Say so rather than showing
        // nothing; `champion.rs` keeps trying.
        state.stats.loading = state.champion.loading;
        state.stats.error = state
            .champion
            .error
            .clone()
            .or_else(|| (!state.champion.loading).then(|| "no data for this champion yet".to_owned()));
        state.stats.offer = None;
        state.stats.pool = None;
        state.stats.upgraded_pool = None;
        return;
    };
    // A card the offer's own pool does not have may be a golden reroll: look one tier up.
    let upgraded = rarity
        .above()
        .filter(|_| key.augments.iter().any(|id| !pool.augments.iter().any(|a| a.id == *id)))
        .and_then(|above| Some((above, state.champion.pool(key.champion_id, above, key.stage)?)))
        .filter(|(_, up)| up.augments.iter().any(|a| key.augments.contains(&a.id)));

    let names = state.static_data.as_deref();
    let pool = rank_pool(pool, rarity, names);
    let upgraded_pool = upgraded.map(|(above, up)| rank_pool(up, above, names));

    let offer = offer_from_pools(&key, &pool, upgraded_pool.as_ref());
    state.stats.loading = false;
    state.stats.error = None;
    state.stats.offer = Some(offer);
    state.stats.pool = Some(pool);
    state.stats.upgraded_pool = upgraded_pool;
    state.stats.key = Some(key);
}

/// One rarity's pool as the overlay shows it: every row with its block, its place in that block
/// and its grade, and named in the game's language.
///
/// The service names augments in English whatever the game's language; the local static data is
/// in the game's locale, so its names replace the service's, matched by id. A row with no local
/// name keeps the service's.
///
/// The pool's order is kept untouched, because that order is the service's ranking.
fn rank_pool(pool: &PoolResponse, rarity: Rarity, names: Option<&static_data::StaticData>) -> PoolStats {
    let blocks: Vec<Block> = pool.augments.iter().map(|a| Block::from_source(&a.source)).collect();
    let augments = pool
        .augments
        .iter()
        .zip(&blocks)
        .zip(ranks_within_blocks(&blocks))
        .map(|((info, &block), place)| {
            let mut info = info.clone();
            let local = names.and_then(|d| d.kiwi.iter().chain(&d.kiwi_jade).find(|p| p.id == info.id));
            if let Some(local) = local {
                info.name = Some(local.name.clone());
            }
            RankedAugment {
                grade: Grade::from_delta(info.delta_pp),
                block,
                pool_rank: place.map(|(rank, _)| rank),
                pool_rank_of: place.map(|(_, of)| of),
                info,
            }
        })
        .collect();
    PoolStats { rarity, augments }
}

/// Picks the offer's cards out of the pool, keeping the pool's order as the ranking.
///
/// With an upgraded card on screen its row comes from `upgraded`, and the two picks are merged by
/// the service's own rule (`sort_best_first`): numbers about this champion first, then all-champion
/// ones, then none, each by delta. Both pools measure against the same champion baseline at the same
/// stage, so their deltas are comparable. The sort only runs in that case, so an ordinary offer
/// still takes the pool's order untouched.
///
/// Each row keeps the place it has in its *own* rarity's list: an upgraded card's rank is its place
/// among the augments of its rarity, not among the offer's.
fn offer_from_pools(key: &StatsKey, pool: &PoolStats, upgraded: Option<&PoolStats>) -> OfferStats {
    let on_screen = |a: &&RankedAugment| key.augments.contains(&a.info.id);
    let mut augments: Vec<_> = pool.augments.iter().filter(on_screen).cloned().collect();
    if let Some(up) = upgraded {
        augments.extend(up.augments.iter().filter(on_screen).cloned());
        let delta = |a: &RankedAugment| a.info.delta_pp.unwrap_or(f64::NEG_INFINITY);
        augments.sort_by(|a, b| a.block.cmp(&b.block).then_with(|| delta(b).total_cmp(&delta(a))));
    }
    let ranking = augments.iter().map(|a| a.info.id).collect();
    OfferStats { augments, ranking }
}

/// The question the current screen poses, if it poses one.
fn current_key(state: &mut EngineState) -> Option<StatsKey> {
    let game = state.game.as_ref()?;
    if !game.is_mayhem {
        return None;
    }
    let offer = game.offers.current()?;

    // The offer picks which ranked list to open. Its rarity is the lowest on screen:
    // an ordinary reroll keeps the rarity and a golden one raises a single card by a tier.
    let augments: Vec<i64> = offer.slots.iter().flatten().copied().collect();
    if augments.is_empty() {
        return None;
    }

    // `champion_key` is locale-independent (`Yasuo`, `LeeSin`), unlike the display name, so it
    // matches CommunityDragon's alias directly.
    let me = game.data.me()?;
    let champion_key = me.champion_key().to_owned();
    // Kept for the log line below, which is the only record of what the live API reported.
    let reported = format!("rawChampionName {:?}, championName {:?}", me.raw_champion_name, me.champion_name);
    let rarity = state.static_data.as_ref().and_then(|d| {
        d.kiwi
            .iter()
            .chain(&d.kiwi_jade)
            .filter(|a| augments.contains(&a.id))
            .filter_map(|a| Rarity::parse(&a.rarity))
            .min()
    });

    let champion_id = state.static_data.as_ref().and_then(|d| d.champion_id(&champion_key));
    let stage = offer.stage;

    match champion_id {
        Some(champion_id) => {
            state.stats.champion_unknown = false;
            Some(StatsKey { champion_id, stage, augments, rarity })
        }
        None => {
            // No id means no lookup. Surfaced rather than silently showing nothing, and logged once
            // with what the live API reported, so the cause can be read from the log.
            if !state.stats.champion_unknown {
                state.log_event(format!("champion unknown: no id for key {champion_key:?} ({reported})"));
            }
            state.stats.champion_unknown = true;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use aramkit_client::AugmentInfo;

    use super::*;

    fn augment(id: i64, delta: f64) -> AugmentInfo {
        AugmentInfo { id, delta_pp: Some(delta), source: "stage".into(), ..Default::default() }
    }

    fn from(source: &str, id: i64, delta: Option<f64>) -> AugmentInfo {
        AugmentInfo { id, delta_pp: delta, source: source.into(), ..Default::default() }
    }

    /// A pool as the service sends it, ranked the way `update` ranks it.
    fn ranked(rarity: Rarity, augments: Vec<AugmentInfo>) -> PoolStats {
        rank_pool(&PoolResponse { augments, ..Default::default() }, rarity, None)
    }

    fn key(augments: &[i64], rarity: Rarity) -> StatsKey {
        StatsKey { champion_id: 157, stage: 2, augments: augments.to_vec(), rarity: Some(rarity) }
    }

    fn static_data(locale: &str, kiwi: Vec<static_data::PoolAugment>) -> static_data::StaticData {
        static_data::StaticData {
            patch: "16.19".into(),
            locale: locale.into(),
            kiwi,
            kiwi_jade: vec![],
            unmatched: vec![],
            champion_ids: Default::default(),
            champion_names: Default::default(),
            offline: false,
        }
    }

    fn local(id: i64, name: &str, rarity: &str) -> static_data::PoolAugment {
        static_data::PoolAugment {
            id,
            name_id: String::new(),
            name: name.into(),
            rarity: rarity.into(),
            icon_url: String::new(),
        }
    }

    #[test]
    fn the_offer_keeps_the_pools_ranking() {
        // The pool arrives sorted by the service's rule, so the cards' order within it is the
        // answer and that ordering never has to be reimplemented here.
        let pool = ranked(Rarity::Gold, vec![augment(10, 5.0), augment(20, 3.0), augment(30, 1.0), augment(40, -2.0)]);

        let offer = offer_from_pools(&key(&[40, 10, 30], Rarity::Gold), &pool, None);
        assert_eq!(offer.ranking, [10, 30, 40], "best first, whatever order the slots were in");
        assert_eq!(offer.augments.len(), 3, "the other pool entries are left out");
    }

    /// What the overlay used to work out for itself, row by row.
    #[test]
    fn every_row_carries_its_block_its_place_there_and_its_grade() {
        let pool = ranked(
            Rarity::Gold,
            vec![
                from("stage", 10, Some(5.0)),
                from("champion", 20, Some(0.2)),
                from("stage", 30, Some(-4.5)),
                // The all-champion fallbacks: a big delta against a different baseline.
                from("global", 40, Some(9.96)),
                from("global", 50, Some(-2.0)),
                from("none", 60, None),
            ],
        );
        let row = |id: i64| pool.augments.iter().find(|a| a.info.id == id).unwrap();
        let place = |id: i64| (row(id).pool_rank, row(id).pool_rank_of);

        assert_eq!((row(10).block, row(10).grade, place(10)), (Block::Champion, Grade::S, (Some(1), Some(3))));
        assert_eq!((row(20).block, row(20).grade, place(20)), (Block::Champion, Grade::B, (Some(2), Some(3))));
        assert_eq!((row(30).block, row(30).grade, place(30)), (Block::Champion, Grade::D, (Some(3), Some(3))));
        // Ranked among the fallbacks only: "#4 / 6" would read as a standing for this champion.
        assert_eq!((row(40).block, row(40).grade, place(40)), (Block::Global, Grade::S, (Some(1), Some(2))));
        assert_eq!(place(50), (Some(2), Some(2)));
        assert_eq!((row(60).block, row(60).grade, place(60)), (Block::None, Grade::Unknown, (None, None)));

        let json = serde_json::to_value(row(10)).unwrap();
        assert_eq!(json["id"], 10, "the service's own fields stay at the top level");
        assert_eq!((json["grade"].as_str(), json["block"].as_str()), (Some("S"), Some("champion")));
        assert_eq!((json["poolRank"].as_u64(), json["poolRankOf"].as_u64()), (Some(1), Some(3)));
    }

    #[test]
    fn pool_names_come_from_the_game_locale() {
        let data = static_data("fr_fr", vec![local(10, "Âme infernale", "kGold")]);
        let response = PoolResponse {
            augments: vec![
                AugmentInfo { name: Some("Infernal Soul".into()), ..augment(10, 1.0) },
                AugmentInfo { name: Some("Unknown".into()), ..augment(99, 0.0) },
            ],
            ..Default::default()
        };
        let pool = rank_pool(&response, Rarity::Gold, Some(&data));
        assert_eq!(pool.augments[0].info.name.as_deref(), Some("Âme infernale"));
        assert_eq!(pool.augments[1].info.name.as_deref(), Some("Unknown"), "no local name: keep the service's");
    }

    #[test]
    fn a_card_the_pool_does_not_have_is_simply_absent() {
        // The overlay renders it with a ? tier and ?? delta rather than inventing numbers.
        let pool = ranked(Rarity::Gold, vec![augment(10, 5.0)]);

        let offer = offer_from_pools(&key(&[10, 999], Rarity::Gold), &pool, None);
        assert_eq!(offer.ranking, [10]);
    }

    #[test]
    fn an_upgraded_card_is_ranked_among_the_others_and_keeps_its_own_lists_place() {
        let gold = ranked(Rarity::Gold, vec![augment(10, 1.9), augment(20, 0.2)]);
        let prismatic = ranked(Rarity::Prismatic, vec![augment(90, 6.0), augment(91, 1.0), augment(92, 0.5)]);

        let offer = offer_from_pools(&key(&[20, 91, 10], Rarity::Gold), &gold, Some(&prismatic));
        assert_eq!(offer.ranking, [10, 91, 20], "merged by delta across the two pools");
        assert_eq!(offer.augments.len(), 3);
        let upgraded = &offer.augments[1];
        assert_eq!((upgraded.pool_rank, upgraded.pool_rank_of), (Some(2), Some(3)), "its place among Prismatics");
    }

    #[test]
    fn a_fallback_never_outranks_a_champion_specific_card_when_two_pools_merge() {
        let gold = ranked(Rarity::Gold, vec![augment(10, 1.0), from("global", 20, Some(9.96))]);
        let prismatic = ranked(Rarity::Prismatic, vec![augment(90, 0.5), from("none", 91, None)]);

        let offer = offer_from_pools(&key(&[20, 90, 10, 91], Rarity::Gold), &gold, Some(&prismatic));
        assert_eq!(offer.ranking, [10, 90, 20, 91], "this champion's numbers, then the fallback, then no data");
    }

    /// The screenshot this was built for: a Gold offer on Aurelion Sol where the golden reroll
    /// turned the middle card into a Prismatic one.
    #[test]
    fn a_golden_reroll_is_answered_from_the_pool_one_tier_up() {
        use crate::engine::champion::ChampionData;
        use crate::engine::GameSession;
        use league_api::lcd::{ActivePlayer, AllGameData, GameData, Player};
        use mayhem_core::augments::{OfferReading, OfferTracker};

        let dir = std::env::temp_dir().join(format!("mayhem-stats-test-{}", std::process::id()));
        let engine = Engine::new(dir.join("config.json"), dir.join("cache"));
        let mut st = engine.lock();

        let mut data = static_data(
            "fr_fr",
            // The Prismatic one first: the offer's rarity must not depend on the list's order.
            vec![
                local(90, "Canon de verre", "kPrismatic"),
                local(10, "Sniper explosif", "kGold"),
                local(20, "Aube ardente", "kGold"),
            ],
        );
        data.champion_ids = [(static_data::champions::normalise_key("AurelionSol"), 136)].into();
        st.set_static_data(data);

        let pool = |rarity: Rarity, augments| PoolResponse {
            rarity: rarity.as_str().into(),
            stage: Some(1),
            augments,
            ..Default::default()
        };
        st.champion = ChampionData {
            champion_id: Some(136),
            pools: [
                ((Rarity::Gold, 1), pool(Rarity::Gold, vec![augment(10, 1.9), augment(20, 0.2)])),
                ((Rarity::Prismatic, 1), pool(Rarity::Prismatic, vec![augment(91, 6.0), augment(90, 3.0)])),
            ]
            .into(),
            ..Default::default()
        };

        let me = Player {
            riot_id: "Me#1".into(),
            raw_champion_name: "game_character_displayname_AurelionSol".into(),
            level: 1,
            ..Default::default()
        };
        let data = AllGameData {
            active_player: ActivePlayer { level: 1, riot_id: "Me#1".into(), ..Default::default() },
            all_players: vec![me],
            game_data: GameData { game_mode: "KIWI".into(), game_time: 20.0, map_number: 12, ..Default::default() },
            ..Default::default()
        };
        let mut offers = OfferTracker::default();
        let reading = OfferReading { slots: [Some(20), Some(90), Some(10)] };
        for t in 0..3 {
            offers.feed(20.0 + f64::from(t), reading, 1);
        }
        assert!(offers.current().is_some(), "the tracker should have opened the offer");
        st.game =
            Some(GameSession { id: 1, clock: Default::default(), last_game_time: 20.0, data, is_mayhem: true, offers });

        update(&mut st);

        let key = st.stats.key.clone().expect("the offer poses a question");
        assert_eq!(key.rarity, Some(Rarity::Gold), "the offer stays Gold: only one card went up");
        assert_eq!(st.stats.pool.as_ref().map(|p| p.rarity), Some(Rarity::Gold));
        let up = st.stats.upgraded_pool.as_ref().expect("the Prismatic pool is held for the upgraded card");
        assert_eq!(up.rarity, Rarity::Prismatic);
        assert_eq!(up.augments[1].info.name.as_deref(), Some("Canon de verre"), "localised like the main pool");
        let offer = st.stats.offer.as_ref().unwrap();
        assert_eq!(offer.ranking, [90, 10, 20], "the upgraded card has numbers and is ranked with the others");
    }
}
