//! The backend engine: a handful of polling loops sharing one state.
//!
//! | Loop | Rate | Owns |
//! |---|---|---|
//! | [`client`] | 1 Hz (4 Hz in queue with auto accept, every 3 s with no client) | LCU discovery, gameflow phase, queue, locale, the champ-select session; triggers static-data loads |
//! | [`game`] | 4 Hz in game, 1 Hz idle | Live Client Data: the game session, clock and augment stage |
//! | [`vision`] | its own thread, 10 Hz | finding and reading the augment and anvil cards |
//! | [`champion`] | 2 Hz check | the locked champion's pools and build, fetched at lock-in |
//! | [`rankings`] | every 6 h | the champion table champ select reads |
//! | [`anvils`] | 1 Hz check | the shard catalogue, the anvil rankings, the enemy damage split |
//! | [`champselect`] | 5 Hz | where the champ-select blocks go |
//! | [`stats`] | 10 Hz | the numbers for the offer on screen, from what was prefetched |
//! | [`itemsets`] | every 600 ms | writing item sets on lock-in |
//!
//! Two more tasks in `lib.rs` publish [`snapshot::AppSnapshot`] to the windows and keep the overlay
//! placed. The state lives behind one `std::sync::Mutex`; nobody holds it across an `.await` or a
//! capture. Every loop is started through [`spawn_task`], so one that dies is reported rather than
//! silently missing.

pub mod anvils;
pub mod champion;
pub mod champselect;
pub mod client;
pub mod game;
pub mod itemsets;
pub mod rankings;
pub mod snapshot;
pub mod stats;
pub mod vision;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use league_api::lcd::AllGameData;
use league_api::lcu::{GameflowPhase, QueueInfo};
use mayhem_core::augments::{AugmentSchedule, OfferTracker};
use mayhem_core::clock::GameClock;
use mayhem_core::queues;
use mayhem_vision::matcher::{AugmentMatcher, AugmentName};
use static_data::StaticData;

use crate::config::{Settings, Tuning, TuningSource};
use snapshot::*;

const EVENT_LOG_LEN: usize = 40;

/// Which window the overlay window is glued to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayTarget {
    None,
    /// The in-game window, for the augment overlay.
    Game,
    /// The League client window, for the champ-select badges.
    Client,
}

pub struct Engine {
    state: Mutex<EngineState>,
    started: Instant,
    pub config_path: PathBuf,
    pub cache_dir: PathBuf,
}

/// Runs one of the engine's loops, and reports it if it ever stops.
///
/// The loops never return, so the task ending means it panicked. Without this the subsystem would
/// simply be gone while the rest of the app carried on and the UI kept showing its last state.
pub fn spawn_task(
    engine: &Arc<Engine>,
    name: &'static str,
    task: impl std::future::Future<Output = ()> + Send + 'static,
) {
    let engine = engine.clone();
    let running = tauri::async_runtime::spawn(task);
    tauri::async_runtime::spawn(async move {
        let why = match running.await {
            Ok(()) => "it returned".to_owned(),
            Err(e) => e.to_string(),
        };
        engine.lock().fault(format!("{name} stopped ({why})"));
    });
}

#[derive(Default)]
pub struct ClientState {
    pub connected: bool,
    /// Kept so the item-set writer can open its own LCU connection without threading a client
    /// through the shared state.
    pub credentials: Option<league_api::lockfile::Credentials>,
    /// The champion the local player has locked in during champ select, if any.
    pub locked_champion: Option<i64>,
    pub phase: Option<GameflowPhase>,
    pub queue: Option<QueueInfo>,
    pub locale: Option<String>,
    pub install_dir: Option<PathBuf>,
    pub error: Option<String>,
}

pub struct GameSession {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub id: u64,
    pub clock: GameClock,
    pub last_game_time: f64,
    pub data: AllGameData,
    pub is_mayhem: bool,
    pub offers: OfferTracker,
}

impl GameSession {
    pub fn stages_unlocked(&self, schedule: &AugmentSchedule) -> u8 {
        if self.is_mayhem {
            schedule.stages_unlocked(self.data.active_player.level)
        } else {
            0
        }
    }
}

#[derive(Default)]
pub struct EngineState {
    /// What the user chose, persisted in `config.json`.
    pub settings: Settings,
    pub settings_dirty: bool,
    /// Fitted and measured constants: the code's defaults, unless `tuning.json` overrides them.
    pub tuning: Tuning,
    /// How `tuning.json` was taken, for the settings screen. `None` when there is none.
    pub tuning_note: Option<String>,
    pub client: ClientState,
    pub game: Option<GameSession>,
    pub next_session_id: u64,
    pub static_data: Option<Arc<StaticData>>,
    pub static_loading: bool,
    pub static_error: Option<String>,
    pub matcher: Option<Arc<AugmentMatcher>>,
    pub vision: VisionView,
    /// Augment statistics for the offer on screen, from our caching service.
    pub stats: stats::StatsState,
    /// Item sets written to the League client.
    pub item_sets: itemsets::ItemSetState,
    /// Everything about the locked champion, fetched during champ select so nothing in game waits
    /// on the network.
    pub champion: champion::ChampionData,
    /// Every champion's rank, tier and rates — one table, fetched once per patch, that champ
    /// select reads for every block on the screen.
    pub rankings: rankings::Rankings,
    /// Stat anvil shards and the hand-authored rankings.
    pub anvils: anvils::AnvilData,
    /// Where the champion icons are on the champ-select screen, so the overlay can label them.
    pub champ_select: champselect::ChampSelectState,
    pub events: VecDeque<String>,
    /// Why the overlay window is, or is not, on screen. Written by the placement loop.
    pub overlay_status: crate::overlay::OverlayStatus,
    /// Parts of the engine that have stopped. See [`EngineState::fault`].
    pub faults: Vec<String>,
    /// Where the log file is, if one could be opened.
    pub log_file: Option<PathBuf>,
}

impl EngineState {
    pub fn log_event(&mut self, text: String) {
        log::info!("{text}");
        if self.events.len() >= EVENT_LOG_LEN {
            self.events.pop_front();
        }
        self.events.push_back(text);
    }

    /// Records that a part of the engine has stopped for good. Shown in the companion window, since
    /// nothing else would give it away: the rest of the app keeps running on the last state.
    pub fn fault(&mut self, what: String) {
        log::error!("{what}");
        if self.events.len() >= EVENT_LOG_LEN {
            self.events.pop_front();
        }
        self.events.push_back(what.clone());
        self.faults.push(what);
    }

    /// Is the running (or upcoming) game Mayhem? In game, the LCU queue is authoritative when
    /// known; the Live Client `gameMode` is the fallback (research check B6).
    pub fn is_mayhem(&self, data: &AllGameData) -> bool {
        let by_queue = self.client.queue.as_ref().is_some_and(|q| queues::is_mayhem_queue(q.id, Some(&q.description)));
        by_queue || queues::is_mayhem_game_mode(&data.game_data.game_mode)
    }

    /// Should the overlay be on screen?
    ///
    /// Not simply "a live session exists": port 2999 only starts answering around the end of the
    /// loading screen, so keying on that left the overlay dark for the whole load. When there is no
    /// session yet, fall back to what the client last told us - the queue is known from champ
    /// select onward, long before the game process is up.
    pub fn overlay_wanted(&self) -> bool {
        if let Some(game) = &self.game {
            return game.is_mayhem;
        }
        let loading = matches!(
            self.client.phase,
            Some(GameflowPhase::GameStart) | Some(GameflowPhase::InProgress) | Some(GameflowPhase::Reconnect)
        );
        loading && self.client.queue.as_ref().is_some_and(|q| queues::is_mayhem_queue(q.id, Some(&q.description)))
    }

    /// Which window the overlay should sit on, if any.
    ///
    /// The game takes precedence: if a Mayhem game is up, that is where the overlay belongs even
    /// during the brief overlap where champ select has not been torn down yet.
    pub fn overlay_target(&self) -> OverlayTarget {
        if !self.settings.overlay_enabled {
            OverlayTarget::None
        } else if self.overlay_wanted() {
            OverlayTarget::Game
        } else if self.champ_select.active() {
            OverlayTarget::Client
        } else {
            OverlayTarget::None
        }
    }

    /// Is an ARAM champ select open? Wider than Mayhem on purpose: the screen this reads has the same
    /// shape in plain ARAM (`queues::has_aram_champ_select`).
    pub fn in_aram_champ_select(&self) -> bool {
        matches!(self.client.phase, Some(GameflowPhase::ChampSelect))
            && self.client.queue.as_ref().is_some_and(|q| queues::has_aram_champ_select(q.id, Some(&q.description)))
    }

    pub fn set_static_data(&mut self, data: StaticData) {
        let mut names: Vec<AugmentName> = Vec::new();
        for a in data.kiwi.iter().chain(&data.kiwi_jade) {
            if !names.iter().any(|n| n.id == a.id) {
                names.push(AugmentName { id: a.id, name: a.name.clone(), rarity: a.rarity.clone() });
            }
        }
        self.matcher = Some(Arc::new(AugmentMatcher::new(self.tuning.matcher, &names)));
        self.log_event(format!(
            "static data {} ({}): {} Mayhem augments{}",
            data.patch,
            data.locale,
            data.kiwi.len(),
            if data.offline { ", offline cache" } else { "" }
        ));
        self.static_data = Some(Arc::new(data));
        self.static_error = None;
    }

    fn card(&self, id: i64) -> AugmentCard {
        let found =
            self.static_data.as_ref().and_then(|d| d.kiwi.iter().chain(&d.kiwi_jade).find(|a| a.id == id).cloned());
        match found {
            Some(a) => AugmentCard { id, name: a.name, rarity: a.rarity },
            None => AugmentCard { id, name: format!("#{id}"), rarity: String::new() },
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let (settings, tuning) = (&self.settings, &self.tuning);
        let client = ClientView {
            connected: self.client.connected,
            phase: self.client.phase.clone().map(String::from),
            queue_id: self.client.queue.as_ref().map(|q| q.id),
            queue_name: self.client.queue.as_ref().map(|q| q.description.clone()),
            is_mayhem_queue: self
                .client
                .queue
                .as_ref()
                .is_some_and(|q| queues::is_mayhem_queue(q.id, Some(&q.description))),
            locale: self.client.locale.clone(),
            // The remembered directory stands in while the client is closed.
            install_dir: self
                .client
                .install_dir
                .as_ref()
                .or(settings.league_dir.as_ref())
                .map(|p| p.display().to_string()),
            error: self.client.error.clone(),
        };

        let game = self.game.as_ref().map(|g| {
            let me = g.data.me();
            GameView {
                game_time: g.last_game_time,
                game_mode: g.data.game_data.game_mode.clone(),
                map_number: g.data.game_data.map_number,
                is_mayhem: g.is_mayhem,
                champion: me.map(|p| p.champion_name.clone()),
                level: g.data.active_player.level,
                is_dead: me.is_some_and(|p| p.is_dead),
                respawn_timer: me.map_or(0.0, |p| p.respawn_timer),
                stages_unlocked: g.stages_unlocked(&tuning.augment_schedule),
            }
        });

        let card_anchors = self.vision.client_size.map(|(w, h)| {
            let client = mayhem_core::geometry::PixelRect::new(0, 0, w, h);
            std::array::from_fn(|i| {
                let layout = &tuning.augment_layout;
                let card = layout.card(&client, i);
                CardAnchor {
                    center_x: (card.x as f32 + card.width as f32 / 2.0) / w.max(1) as f32,
                    panel_bottom_y: layout.panel_bottom_y(&client) / h.max(1) as f32,
                    anvil_bottom_y: layout.anvil_label_bottom_y(&client) / h.max(1) as f32,
                    width: card.width as f32 * layout.panel_width / w.max(1) as f32,
                }
            })
        });

        let augments = match &self.game {
            Some(g) if g.is_mayhem => {
                let unlocked = g.stages_unlocked(&tuning.augment_schedule);
                let offer = g
                    .offers
                    .current()
                    .map(|o| OfferView { stage: o.stage, cards: o.slots.map(|s| s.map(|id| self.card(id))) });
                AugmentsView {
                    offer,
                    seen_augment_ids: g.offers.seen_augments().iter().copied().collect(),
                    stages_seen: g.offers.stages_seen(),
                    offer_due: unlocked > g.offers.stages_seen(),
                    ocr_blocker: self.ocr_blocker(),
                    card_anchors,
                }
            }
            _ => AugmentsView { ocr_blocker: self.ocr_blocker(), card_anchors, ..Default::default() },
        };

        let stats = StatsView {
            offer: self.stats.offer.clone(),
            pool: self.stats.pool.clone(),
            upgraded_pool: self.stats.upgraded_pool.clone(),
            freshness: self.champion.freshness,
            loading: self.stats.loading,
            error: self.stats.error.clone(),
            champion_unknown: self.stats.champion_unknown,
        };

        let champ_select = {
            let cs = &self.champ_select;
            let slots = match cs.client_rect {
                Some(parent) => cs
                    .slots
                    .iter()
                    // Every slot already has a champion: a block whose occupant is unknown is never
                    // built in the first place, and the reason is reported once in `mismatch` rather
                    // than repeated as a row of blanks across the screen.
                    .map(|s| {
                        // The overlay covers the client area exactly, so normalising against it is
                        // what makes these CSS percentages.
                        let n = mayhem_core::geometry::NormRect::from_pixels(&s.rect, &parent);
                        ChampSelectSlotView {
                            surface: s.surface,
                            index: s.index,
                            champion_id: s.champion_id,
                            stats: s.stats.clone(),
                            swappable: s.swappable,
                            x: n.x,
                            y: n.y,
                            width: n.width,
                            height: n.height,
                        }
                    })
                    .collect(),
                // Without the client area there is nothing to normalise against, and a badge placed
                // against a guess would be worse than no badge.
                None => Vec::new(),
            };
            ChampSelectView {
                active: cs.active(),
                card_count: cs.offered().len(),
                bench_size: cs.session.as_ref().map_or(0, |s| s.bench.len()),
                subset_size: cs.session.as_ref().map_or(0, |s| s.subset.len()),
                cards_settled: cs.cards_settled(),
                time_left_ms: cs.session.as_ref().map_or(0, |s| s.time_left_ms),
                phase_total_ms: cs.session.as_ref().map_or(0, |s| s.phase_total_ms),
                phase: cs.session.as_ref().map(|s| s.phase.clone()).unwrap_or_default(),
                slots,
                stale: cs.stale,
                last_swap: cs.last_swap.clone(),
                data_patch: self.rankings.patch.clone(),
                data_date: self.rankings.data_date.clone(),
                freshness: self.rankings.freshness,
                rankings_error: self.rankings.error.clone(),
                ranked_champions: self.rankings.by_id.len(),
            }
        };

        let champion = ChampionPrepView {
            champion_id: self.champion.champion_id,
            ready: self.champion.champion_id.is_some_and(|id| self.champion.is_ready(id)),
            loading: self.champion.loading,
            pools_loaded: self.champion.pools.len(),
            pools_expected: champion::RARITIES.len() * champion::STAGES.len(),
            build_loaded: self.champion.build.is_some(),
            error: self.champion.error.clone(),
        };

        let static_data = match &self.static_data {
            Some(d) => StaticDataView {
                loaded: true,
                patch: Some(d.patch.clone()),
                locale: Some(d.locale.clone()),
                pool_size: d.kiwi.len(),
                offline: d.offline,
                error: self.static_error.clone(),
            },
            None => StaticDataView { error: self.static_error.clone(), ..Default::default() },
        };

        let anvil = self.anvil_view();
        let mut vision = self.vision.clone();
        vision.recent_events = self.events.iter().rev().take(15).cloned().collect();
        AppSnapshot {
            client,
            game,
            augments,
            overlay_active: self.overlay_wanted(),
            options: OverlayOptions {
                simple_mode: settings.simple_mode,
                show_augment_list: settings.show_augment_list,
            },
            champion,
            champ_select,
            overlay: self.overlay_status,
            stats,
            vision,
            static_data,
            anvil,
            diagnostics: DiagnosticsView {
                faults: self.faults.clone(),
                log_file: self.log_file.as_ref().map(|p| p.display().to_string()),
                tuning: self.tuning_note.clone(),
            },
        }
    }

    /// The anvil offer on screen with each card's rank for the champion.
    ///
    /// The rank is the shard's dense rank in the whole tier's pool, not among the three cards, so
    /// the same shard always shows the same number for the same champion.
    fn anvil_view(&self) -> snapshot::AnvilView {
        let data = &self.anvils;
        // Decided here rather than where the ranks are built, so the settings screen can say which
        // way a tie will go before an anvil is on screen.
        let resist_first =
            data.enemy_damage.as_ref().map(|d| mayhem_core::anvils::Resist::against(d.split.physical, d.split.magic));
        let mut view = snapshot::AnvilView {
            enemy_damage: data.enemy_damage.as_ref().map(|d| d.split),
            resist_first,
            shards_loaded: data.catalogue.len(),
            rankings_saved_at: data.rankings.as_ref().and_then(|r| r.saved_at),
            error: data.error(),
            ..Default::default()
        };
        let Some(read) = &self.vision.anvil else { return view };
        view.on_screen = true;
        view.tier = read.decision.tier;

        let Some(tier) = read.decision.tier else {
            view.status = read.decision.problem.clone().or(Some("reading the shard values".into()));
            return view;
        };
        let champion = champion::wanted_champion(self);
        let group = champion.and_then(|id| data.rankings.as_ref()?.group_of(id));
        view.group = group.map(|g| g.name.clone());
        let tier_name = match tier {
            mayhem_core::anvils::AnvilTier::Silver => "silver",
            mayhem_core::anvils::AnvilTier::Gold => "gold",
            mayhem_core::anvils::AnvilTier::Prismatic => "prismatic",
        };
        let ranks = group
            .map(|g| {
                let buckets = g.tiers.get(tier_name);
                match resist_first {
                    Some(first) => mayhem_core::anvils::dense_ranks(&mayhem_core::anvils::break_resist_ties(
                        buckets,
                        &data.catalogue,
                        first,
                    )),
                    None => mayhem_core::anvils::dense_ranks(buckets),
                }
            })
            .unwrap_or_default();
        view.rank_of = ranks.values().copied().max();

        let mut cards: Vec<Option<snapshot::AnvilCardView>> = read
            .decision
            .shards
            .iter()
            .map(|id| {
                let id = id.as_ref()?;
                let shard = data.catalogue.iter().find(|s| s.id == *id && s.tier == tier)?;
                Some(snapshot::AnvilCardView {
                    id: id.clone(),
                    name: shard.name.clone(),
                    rank: ranks.get(id).copied(),
                    best: false,
                })
            })
            .collect();
        if let Some(top) = cards.iter().flatten().filter_map(|c| c.rank).min() {
            for card in cards.iter_mut().flatten() {
                card.best = card.rank == Some(top);
            }
        }
        view.cards = cards;

        view.status = match (champion, &data.rankings, group) {
            (_, None, _) => Some("anvil rankings not loaded".into()),
            (None, _, _) => Some("champion unknown".into()),
            (Some(id), _, None) => {
                let name = self.static_data.as_ref().and_then(|d| d.champion_names.get(&id).cloned());
                Some(format!("no anvil ranking for {}", name.unwrap_or_else(|| format!("champion {id}"))))
            }
            _ => None,
        };
        view
    }

    fn ocr_blocker(&self) -> Option<String> {
        if !self.vision.available {
            return self.vision.unavailable_reason.clone().or(Some("screen reading unavailable".into()));
        }
        if self.matcher.is_none() {
            return Some("augment names not loaded (static data)".into());
        }
        None
    }
}

impl Engine {
    pub fn new(config_path: PathBuf, cache_dir: PathBuf) -> Self {
        let settings = Settings::load(&config_path);
        let tuning_path = Tuning::path_beside(&config_path);
        let (tuning, source) = Tuning::load(&tuning_path);
        let mut state = EngineState { settings, tuning, ..Default::default() };
        // Said out loud either way: an override changes how the app behaves, and one that was
        // ignored is a file somebody expected to matter.
        match source {
            TuningSource::Defaults => {}
            TuningSource::Overridden => {
                state.tuning_note = Some("applied".into());
                state.log_event(format!("tuning overridden by {}", tuning_path.display()));
            }
            TuningSource::Invalid(why) => {
                state.tuning_note = Some(format!("ignored: {why}"));
                log::warn!("{} is invalid and was ignored: {why}", tuning_path.display());
                state.log_event(format!("tuning.json ignored: {why}"));
            }
        }
        Self { state: Mutex::new(state), started: Instant::now(), config_path, cache_dir }
    }

    /// Monotonic seconds since start; the "local" side of [`GameClock`].
    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn lock(&self) -> MutexGuard<'_, EngineState> {
        // A panic while holding the lock leaves plain data behind; keep serving it.
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn save_settings_if_dirty(&self) {
        let settings = {
            let mut st = self.lock();
            if !st.settings_dirty {
                return;
            }
            st.settings_dirty = false;
            st.settings.clone()
        };
        if let Err(e) = settings.save(&self.config_path) {
            log::warn!("could not save the settings: {e}");
            self.lock().settings_dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(id: i64, description: &str) -> QueueInfo {
        QueueInfo { id, description: description.into(), game_mode: String::new(), map_id: 12 }
    }

    /// The overlay used to wait for a live session, which meant waiting for port 2999 - it only
    /// answers near the end of the loading screen, so the overlay was dark for the whole load.
    #[test]
    fn the_overlay_comes_up_during_the_loading_screen() {
        let mut st = EngineState::default();
        assert!(!st.overlay_wanted(), "nothing is running");

        // Champ select: the queue is known, but we are not in a game yet.
        st.client.queue = Some(queue(0, "ARAM Mayhem"));
        st.client.phase = Some(GameflowPhase::ChampSelect);
        assert!(!st.overlay_wanted());

        // Loading screen: no session yet, and the overlay must already be up.
        st.client.phase = Some(GameflowPhase::GameStart);
        assert!(st.overlay_wanted());
        st.client.phase = Some(GameflowPhase::InProgress);
        assert!(st.overlay_wanted());

        // A non-Mayhem game never gets an overlay.
        st.client.queue = Some(queue(420, "Ranked Solo"));
        assert!(!st.overlay_wanted());

        // And it goes away once the game is over.
        st.client.queue = Some(queue(0, "ARAM Mayhem"));
        st.client.phase = Some(GameflowPhase::WaitingForStats);
        assert!(!st.overlay_wanted());
    }

    /// The saved rankings tie Armor with Magic Resist. The enemy team's damage decides which goes
    /// first, and with no split known the tie stands.
    #[test]
    fn the_enemy_team_breaks_an_armor_and_magic_resist_tie() {
        use aramkit_client::{AnvilGroup, AnvilRankings, AnvilTiers, DamageSplit};
        use mayhem_core::anvils::{AnvilDecision, AnvilTier, Resist, ShardInfo};

        let shard = |id: &str, kind: &str| ShardInfo {
            id: id.into(),
            tier: AnvilTier::Gold,
            kind: kind.into(),
            name: id.into(),
            values: vec![45.0],
        };
        let mut st = EngineState::default();
        st.client.locked_champion = Some(99);
        st.anvils.catalogue = Arc::new(vec![shard("G_HP", "MinHP"), shard("G_AR", "MinAR"), shard("G_MR", "MinMR")]);
        st.anvils.rankings = Some(AnvilRankings {
            groups: vec![AnvilGroup {
                name: "Mages".into(),
                champions: vec![99],
                tiers: AnvilTiers {
                    gold: vec![vec!["G_HP".into()], vec!["G_AR".into(), "G_MR".into()]],
                    ..Default::default()
                },
            }],
            ..Default::default()
        });
        st.vision.anvil = Some(mayhem_vision::anvil::AnvilRead {
            title_texts: Default::default(),
            value_texts: Default::default(),
            matches: Default::default(),
            decision: AnvilDecision {
                tier: Some(AnvilTier::Gold),
                shards: [Some("G_HP".into()), Some("G_AR".into()), Some("G_MR".into())],
                problem: None,
            },
        });
        let ranks = |st: &EngineState| -> Vec<Option<u32>> {
            st.anvil_view().cards.iter().map(|c| c.as_ref().and_then(|c| c.rank)).collect()
        };
        let facing = |physical: f64, magic: f64| {
            Some(anvils::EnemyDamage { champions: vec![1], split: DamageSplit { physical, magic, true_damage: 0.05 } })
        };

        assert_eq!(ranks(&st), [Some(1), Some(2), Some(2)], "no split known: the tie stands");
        assert_eq!(st.anvil_view().resist_first, None);

        st.anvils.enemy_damage = facing(0.4489, 0.5015);
        assert_eq!(ranks(&st), [Some(1), Some(3), Some(2)], "more magic: Magic Resist first");
        assert_eq!(st.anvil_view().resist_first, Some(Resist::MagicResist));

        st.anvils.enemy_damage = facing(0.475, 0.475);
        assert_eq!(ranks(&st), [Some(1), Some(2), Some(3)], "an exact tie goes to Armor");

        st.anvils.enemy_damage = facing(0.70, 0.25);
        assert_eq!(ranks(&st), [Some(1), Some(2), Some(3)], "more physical: Armor first");
    }

    /// The Enabled checkbox: off means no host at all, whatever is running.
    #[test]
    fn a_disabled_overlay_has_no_host() {
        let mut st = EngineState::default();
        st.client.queue = Some(queue(0, "ARAM Mayhem"));
        st.client.phase = Some(GameflowPhase::InProgress);
        assert_eq!(st.overlay_target(), OverlayTarget::Game);
        st.settings.overlay_enabled = false;
        assert_eq!(st.overlay_target(), OverlayTarget::None);
    }

    /// One overlay window, two possible hosts. Which one it is glued to is decided here, and getting
    /// it wrong means the window is owned by the wrong process: placed over the client during a game,
    /// or over the game during champ select.
    #[test]
    fn the_overlay_follows_champ_select_to_the_client_and_the_game_to_the_game() {
        let mut st = EngineState::default();
        assert_eq!(st.overlay_target(), OverlayTarget::None, "nothing is running");

        // ARAM champ select: the badges belong on the client window.
        st.client.queue = Some(queue(450, "ARAM"));
        st.client.phase = Some(GameflowPhase::ChampSelect);
        st.champ_select.session = Some(league_api::lcu::ChampSelect {
            local_champion: Some(22),
            team: vec![Some(22), Some(64)],
            bench: vec![84],
            phase: "BAN_PICK".into(),
            ..Default::default()
        });
        assert!(st.in_aram_champ_select());
        assert_eq!(st.overlay_target(), OverlayTarget::Client);

        // A draft queue has no bench and no screen we know how to read.
        st.client.queue = Some(queue(420, "Ranked Solo"));
        assert!(!st.in_aram_champ_select());

        // The loading screen starts: the overlay moves to the game, even though the champ-select
        // session has not been cleared yet, because the game always wins.
        st.client.queue = Some(queue(2400, "ARAM: Mayhem"));
        st.client.phase = Some(GameflowPhase::GameStart);
        assert_eq!(st.overlay_target(), OverlayTarget::Game);

        // Champ select over and no game: nowhere to be.
        st.champ_select.clear();
        st.client.phase = Some(GameflowPhase::WaitingForStats);
        assert_eq!(st.overlay_target(), OverlayTarget::None);
    }
}
