//! What the frontends see. Emitted as the `snapshot` event several times a second and available
//! on demand through the `get_snapshot` command. Mirrored in `src/types.ts`.

use mayhem_core::augments::OFFER_SLOTS;
use mayhem_core::ranking::{Block, Grade, Rarity};
use mayhem_vision::frames::FrameMatch;
use mayhem_vision::reroll::RerollMatch;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub client: ClientView,
    pub game: Option<GameView>,
    pub augments: AugmentsView,
    /// The backend believes the overlay should be on screen (including through the loading screen,
    /// before the Live Client API answers).
    pub overlay_active: bool,
    /// How the overlay draws, from the companion window's checkboxes.
    pub options: OverlayOptions,
    pub champion: ChampionPrepView,
    pub champ_select: ChampSelectView,
    /// Why the overlay window is, or is not, on screen.
    pub overlay: crate::overlay::OverlayStatus,
    pub stats: StatsView,
    pub vision: VisionView,
    pub static_data: StaticDataView,
    /// The downloaded statistics every champion's numbers come from.
    pub dataset: DatasetView,
    /// The stat anvil offer on screen, ranked for the champion.
    pub anvil: AnvilView,
    pub diagnostics: DiagnosticsView,
}

/// What a bug report needs: whether a part of the app has stopped, and where the log is.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsView {
    /// Parts of the engine that have stopped running, in the order they stopped. Empty in a healthy
    /// app. Nothing restarts them: the app needs restarting, and the log says why they stopped.
    pub faults: Vec<String>,
    /// The log file, or `None` when it could not be opened.
    pub log_file: Option<String>,
    /// How `tuning.json` was taken: `null` when there is none, `"applied"`, or why it was ignored.
    pub tuning: Option<String>,
}

/// The overlay's display settings. Carried in the snapshot so a checkbox takes effect on the next
/// frame, without the overlay window having to ask for the config.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayOptions {
    pub simple_mode: bool,
    pub show_augment_list: bool,
}

/// What the overlay draws on a stat anvil offer.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnvilView {
    /// The cards on screen were read as shards, not augments.
    pub on_screen: bool,
    pub tier: Option<mayhem_core::anvils::AnvilTier>,
    /// The champion's ranking group. `None` means no labels: there is no fallback group.
    pub group: Option<String>,
    /// Left to right. Empty unless `on_screen`.
    pub cards: Vec<Option<AnvilCardView>>,
    /// The worst rank in this tier's ranking, so a card reads `#2 / 6`. Ranks are dense, so this is
    /// the number of distinct places. `None` when nothing is ranked.
    pub rank_of: Option<u32>,
    /// Why no labels are drawn, when they are not.
    pub status: Option<String>,
    /// Shards known for this patch and locale; 0 until the catalogue arrives.
    pub shards_loaded: usize,
    pub rankings_saved_at: Option<i64>,
    pub error: Option<String>,
    /// How the enemy team deals its damage, once known.
    pub enemy_damage: Option<aramkit_client::DamageSplit>,
    /// Which of Armor and Magic Resist goes first where the rankings tie them. `None` leaves the tie.
    pub resist_first: Option<mayhem_core::anvils::Resist>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnvilCardView {
    pub id: String,
    pub name: String,
    /// Dense rank in the whole pool for this tier, `1` best. `None` when the shard is unranked.
    pub rank: Option<u32>,
    /// The best-ranked card on screen; ties share it.
    pub best: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientView {
    pub connected: bool,
    pub phase: Option<String>,
    pub queue_id: Option<i64>,
    pub queue_name: Option<String>,
    pub is_mayhem_queue: bool,
    pub locale: Option<String>,
    pub install_dir: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    pub game_time: f64,
    pub game_mode: String,
    pub map_number: i64,
    pub is_mayhem: bool,
    pub champion: Option<String>,
    pub level: u8,
    pub is_dead: bool,
    pub respawn_timer: f64,
    pub stages_unlocked: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AugmentCard {
    pub id: i64,
    pub name: String,
    /// CommunityDragon's spelling, `kGold`.
    pub rarity: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AugmentsView {
    /// The offer on screen right now, if any.
    pub offer: Option<OfferView>,
    pub stages_seen: u8,
    /// An unlocked offer has not been seen yet (the Option A "offer due" signal).
    pub offer_due: bool,
    /// Why OCR cannot run, if it cannot.
    pub ocr_blocker: Option<String>,
    /// Where the three cards are on screen. `None` until the game window size is known.
    pub card_anchors: Option<[CardAnchor; OFFER_SLOTS]>,
    /// Every augment seen on screen this game. They cannot be offered again, so the list strikes
    /// them through.
    pub seen_augment_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferView {
    pub stage: u8,
    pub cards: [Option<AugmentCard>; OFFER_SLOTS],
}

/// Where one card sits, normalised to the client area (0..1).
///
/// The overlay window covers the client area exactly, so these are directly CSS percentages. They
/// live here rather than being recomputed in TypeScript because the card layout is measured, tuned
/// and tested in `mayhem-core`, and two copies of it would drift.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAnchor {
    /// Horizontal centre of the card frame.
    pub center_x: f32,
    /// Where a panel's **bottom edge** goes. Panels are anchored by their bottom and grow upwards,
    /// because the reroll button sits just below and anything growing downwards would cover it.
    pub panel_bottom_y: f32,
    /// Where a stat anvil label's bottom edge goes: inside the card, just above its bottom frame.
    pub anvil_bottom_y: f32,
    /// The panel's width: the card frame's width scaled by `AugmentLayout::panel_width`.
    pub width: f32,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrDebug {
    pub texts: [String; OFFER_SLOTS],
    pub scores: [Option<f32>; OFFER_SLOTS],
    pub at_game_time: f64,
    pub millis: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionView {
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub ocr_engine: Option<String>,
    pub game_window_found: bool,
    pub client_size: Option<(u32, u32)>,
    pub samples_per_second: f64,
    pub last_error: Option<String>,
    /// The reroll buttons, left to right: the first and cheapest thing the pipeline looks for, and
    /// the one signal the game's augment tooltip cannot cover.
    pub rerolls: Vec<RerollView>,
    /// The "hide augments" button, which stays on screen when the cards are put away.
    pub button: ButtonView,
    /// Card frame matches from the last tick, left to right. Empty outside an offer, because the
    /// card area is not captured unless something cheaper said there was a reason to.
    pub cards: Vec<CardFrameView>,
    /// The cards are drawn: any reroll button, or any card frame.
    pub cards_on_screen: bool,
    pub last_ocr: Option<OcrDebug>,
    /// The raw anvil reading (texts, numbers' verdict), for the settings screen.
    pub anvil: Option<mayhem_vision::anvil::AnvilRead>,
    pub recent_events: Vec<String>,
}

/// How well the bundled "hide augments" template matched. Surfaced because it is the fallback gate:
/// when the rerolls are absent, this is what says whether an offer is up with the cards put away.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonView {
    pub score: f32,
    pub present: bool,
}

/// How well a reroll button matched. This is the first gate the pipeline passes through, so when
/// nothing is detected these scores say whether the problem is before or after it.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerollView {
    pub score: f32,
    pub present: bool,
    /// The reroll has been used. Nothing depends on it; it is here to read.
    pub spent: bool,
}

impl From<&RerollMatch> for RerollView {
    fn from(m: &RerollMatch) -> Self {
        Self { score: m.score, present: m.present, spent: m.spent }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardFrameView {
    pub score: f32,
    pub present: bool,
}

impl From<&FrameMatch> for CardFrameView {
    fn from(m: &FrameMatch) -> Self {
        Self { score: m.score, present: m.present }
    }
}

/// Progress of the champ select prefetch, so the standby chip can say whether the first offer will
/// be instant.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionPrepView {
    pub champion_id: Option<i64>,
    /// Every pool and the build have arrived.
    pub ready: bool,
    pub loading: bool,
    pub pools_loaded: usize,
    pub pools_expected: usize,
    pub build_loaded: bool,
    pub error: Option<String>,
}

/// The champ-select screen: where every badge goes, and who is in it where that is known.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectView {
    /// An ARAM champ select is open, so the overlay belongs on the client window.
    pub active: bool,
    /// Champions on the big cards right now: two or three during `BAN_PICK`, none afterwards.
    pub card_count: usize,
    /// Champions on the bench, which is what the available-champions strip shows.
    pub bench_size: usize,
    /// Champions the subset endpoint offers, whether or not their blocks are drawn yet. Diagnostic:
    /// with `cardCount` and `cardsSettled` it says *why* there are no card blocks — no offer at all,
    /// or an offer still sliding into place.
    pub subset_size: usize,
    /// The cards have been up long enough for the entrance animation to have finished.
    pub cards_settled: bool,
    /// `timer.adjustedTimeLeftInPhase`, reported to be looked at only — nothing is gated on it.
    pub time_left_ms: i64,
    /// `timer.totalTimeInPhase`, or `0` when the client does not report it. Undocumented field, so
    /// this is here to be *looked at*, not relied on.
    pub phase_total_ms: i64,
    /// The champ-select phase the client reports: `PLANNING`, `BAN_PICK`, `FINALIZATION`.
    pub phase: String,
    pub slots: Vec<ChampSelectSlotView>,
    /// The session on screen is the last one that read successfully, not a fresh one.
    pub stale: bool,
    /// How the last swap went, for the status line.
    pub last_swap: Option<String>,

    // -- where the numbers came from. The overlay showed a PLACEHOLDER warning while they were
    // -- invented; now that they are real, it says which patch they are for instead.
    /// The aramkit patch the champion table is for, empty before it has been fetched.
    pub data_patch: String,
    /// The date aramkit built the data.
    pub data_date: String,
    /// `stale` when the table came off the on-disk copy because the service was unreachable.
    pub freshness: Option<aramkit_client::Freshness>,
    /// Why the champion table is missing or old, if it is.
    pub rankings_error: Option<String>,
    /// Champions in the table. Zero means every block is drawn without figures, which the overlay
    /// says out loud rather than leaving to be inferred from empty blocks.
    pub ranked_champions: usize,
}

/// One statistics block on the champ-select screen.
///
/// Normalised to the client window's client area, which the overlay window covers exactly, so these
/// are directly CSS percentages -- the same contract as [`CardAnchor`].
///
/// There is deliberately no champion *name* here. The client already prints the name next to every
/// one of these, and repeating it would spend the block's limited room saying something the player
/// can already read.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectSlotView {
    /// `pickCard`, `strip` or `ally`.
    pub surface: super::champselect::Surface,
    pub index: usize,
    /// The champion this block describes. Blocks are only produced where this is known.
    pub champion_id: i64,
    /// `null` when the champion table has no row for this champion, or has not arrived yet. The
    /// block is still drawn — on the bench it is the swap button — with no figures in it.
    pub stats: Option<super::champselect::ChampionStats>,
    /// Clicking this block swaps to that champion.
    pub swappable: bool,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One augment as the overlay draws it: the service's row, plus every judgement made about it.
///
/// The service's fields are carried unchanged and flattened in, so the wire contract still has one
/// definition. What is added is decided in [`mayhem_core::ranking`], so the overlay never ranks,
/// grades or classifies anything itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedAugment {
    #[serde(flatten)]
    pub info: aramkit_client::AugmentInfo,
    /// Which baseline the delta is against, and so which list the row is ranked in.
    pub block: Block,
    /// S to D from the delta, `?` when there is none.
    pub grade: Grade,
    /// The row's place within its block, among the augments of its own rarity, `1` best. `None`
    /// for a row with no data.
    pub pool_rank: Option<u32>,
    /// What `pool_rank` is out of.
    pub pool_rank_of: Option<u32>,
}

/// Every augment of one rarity for the champion, in the service's ranked order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolStats {
    pub rarity: Rarity,
    pub augments: Vec<RankedAugment>,
}

/// The cards on screen, best first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferStats {
    pub augments: Vec<RankedAugment>,
    /// Augment ids best first. The overlay highlights the first.
    pub ranking: Vec<i64>,
}

/// Augment statistics for the offer on screen, from our caching service.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsView {
    /// The three cards with their win rates, deltas and per-stage numbers, best first.
    pub offer: Option<OfferStats>,
    /// The ranked list for this offer's rarity, which the overlay opens top right.
    pub pool: Option<PoolStats>,
    /// The list one rarity up, present only while a card on screen is a golden reroll: that
    /// card's rank is its place in this list, not in `pool`.
    pub upgraded_pool: Option<PoolStats>,
    /// `live` or `stale`; `stale` means the service was unreachable and this is the stored copy.
    pub freshness: Option<aramkit_client::Freshness>,
    pub loading: bool,
    pub error: Option<String>,
    /// An offer is on screen but the champion could not be identified, so no lookup is possible.
    pub champion_unknown: bool,
}

/// The downloaded statistics (D-090): what is on disk, and the download if one is running.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetView {
    /// A dataset is on disk and in use. Nothing is shown until there is one.
    pub loaded: bool,
    pub patch: Option<String>,
    pub data_date: Option<String>,
    pub champions: usize,
    /// When the dataset in use was downloaded, Unix seconds.
    pub downloaded_at: Option<u64>,
    /// When the service last confirmed it or sent a newer one, Unix seconds; `None` since start.
    pub checked_at: Option<u64>,
    /// `[received, total]` bytes while a download runs; `total` is `None` when unknown.
    pub downloading: Option<(u64, Option<u64>)>,
    /// Why the last update failed. With a dataset loaded, the app carries on with it.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticDataView {
    pub loaded: bool,
    pub patch: Option<String>,
    pub locale: Option<String>,
    pub pool_size: usize,
    pub offline: bool,
    pub error: Option<String>,
}
