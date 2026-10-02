//! The ARAM: Mayhem champ-select overlay: where the statistics blocks go, and who each one is about.
//!
//! **Entirely LCU-driven.** Nothing here reads the screen. Positions come from a layout
//! fitted to real captures ([`mayhem_core::champselect`]), and every piece of *state* comes from the
//! champ-select session:
//!
//! | What | Where from |
//! |---|---|
//! | Are the big cards up? | `timer.phase == BAN_PICK` |
//! | Which champions are on them | the subset list |
//! | What can be swapped to | `benchChampions` |
//! | Which teammate is in which row | `myTeam`, by `cellId` |
//!
//! An earlier version detected the cards and the occupied strip slots by sampling pixels. It worked
//! on still captures and was wrong in motion: a capture could fail, arrive mid-animation, or be taken
//! while another window covered the client, and every one of those blanked the blocks for a moment.
//! The overlay flickered. None of that information needed reading in the first place — the client
//! already publishes all of it — so the screen reading is gone rather than debounced. There is
//! nothing left to flicker.

use std::sync::Arc;
use std::time::{Duration, Instant};

use league_api::lcu::ChampSelect;
use mayhem_core::champselect::ChampSelectLayout;
use mayhem_core::geometry::PixelRect;

use super::Engine;

/// The overlay follows the client's own polling; there is no capture to pace against any more.
const POLL: Duration = Duration::from_millis(200);

/// How long the client spends sliding and scaling the pick cards into place when `BAN_PICK` opens.
///
/// A block pinned to where a card *will* be sits in empty space until the card arrives, so the card
/// blocks wait this out and then stay up for the whole rest of the phase.
///
/// Timed from **our own clock**, from the first tick that saw the cards, not from the phase timer.
/// Deriving elapsed time from `adjustedTimeLeftInPhase` needs the phase's length, and the only field
/// that would give it (`totalTimeInPhase`) is not in the LCU specification — when it is missing or
/// disagrees, the arithmetic reads as "the phase has barely started" for the phase's whole duration
/// and no card block is ever drawn. Two seconds after we first see the cards is two seconds
/// whatever the client reports.
const CARDS_ENTRANCE: Duration = Duration::from_secs(2);

/// Which surface a block belongs to.
///
/// Worth distinguishing because they mean different things: a pick card is a decision being made
/// right now, a strip slot is a champion you can swap to, an ally row is information about the team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Surface {
    PickCard,
    Strip,
    Ally,
}

/// Champion statistics for one block: what the block is actually for.
///
/// Real aramkit numbers, by way of our caching service: the whole champion table is fetched
/// once per patch into [`super::rankings`] and looked up here. A champion the table has no row for
/// gets **no block statistics at all** rather than invented ones — see [`stats_for`].
///
/// # Units
///
/// The service reports rates as fractions, the way aramkit does (`0.5778`). These are
/// **percentages** (`57.78`), converted once in [`stats_for`], because every consumer of this struct
/// is a piece of user interface that shows a percentage. Nothing downstream multiplies again.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionStats {
    /// aramkit's tier letter, `None` when they publish a row with no tier.
    pub tier: Option<String>,
    /// aramkit's own rank; 1 is best. Passed through rather than derived from the win rate, because
    /// their ordering is weighted and is what the tier beside it agrees with.
    pub rank: i64,
    /// How many champions the rank is out of — the `/ 173` in `# 3 / 173`. A rank means nothing
    /// without it: third of 173 and third of 5 are not the same claim.
    pub pool_size: i64,
    /// Win rate as a percentage.
    pub win_rate: f32,
    /// Pick rate as a percentage.
    pub pick_rate: f32,
    /// Games behind these numbers, so the overlay can warn on a thin row.
    pub sample_count: i64,
}

/// The block statistics for one champion, or `None` when the table has no row for them.
///
/// `None` is a real case rather than a defensive one: a champion released this patch, or reworked
/// enough that aramkit drops them, has no row, and so does every champion while the table is still
/// being fetched or when the service has never been reachable. The block is still drawn — on the
/// bench it is the swap button, so removing it would remove the ability to swap — it simply shows no
/// figures. Inventing a plausible-looking win rate is the one thing that must not happen here.
fn stats_for(rankings: &super::rankings::Rankings, champion_id: i64) -> Option<ChampionStats> {
    let row = rankings.get(champion_id)?;
    if rankings.pool_size <= 0 {
        // A rank with nothing to measure it against is not worth showing.
        return None;
    }
    Some(ChampionStats {
        tier: row.tier.clone(),
        rank: row.rank,
        pool_size: rankings.pool_size,
        // Fractions upstream, percentages here. The only place this conversion happens.
        win_rate: (row.win_rate * 100.0) as f32,
        pick_rate: (row.pick_rate * 100.0) as f32,
        sample_count: row.sample_count,
    })
}

/// One statistics block: where it is drawn, and who it is about.
#[derive(Debug, Clone)]
pub struct Slot {
    pub surface: Surface,
    /// Position within its surface, left to right or top to bottom. For an ally this is the seat, so
    /// it is the row the client drew them in.
    pub index: usize,
    /// The block itself, in **physical screen pixels** — the layout is resolved against the client
    /// window's rectangle on the desktop. The snapshot subtracts that origin to normalise, and the
    /// pointer is tracked in the same coordinates, so neither side re-offsets.
    pub rect: PixelRect,
    /// The champion this block describes. A block is only produced where this is known, so there is
    /// never an empty one sitting on the screen.
    pub champion_id: i64,
    /// `None` when the champion table has no row for this champion, or has not arrived yet. The
    /// block is still drawn — on the bench it is the swap button — it just shows no figures.
    pub stats: Option<ChampionStats>,
    /// Clicking this block swaps to that champion. Only the strip qualifies: it is the bench, and the
    /// bench is the only thing a swap can target.
    pub swappable: bool,
}

#[derive(Default)]
pub struct ChampSelectState {
    /// What the LCU last reported, or `None` when champ select is not open.
    pub session: Option<ChampSelect>,
    /// The client window's client area. Blocks are normalised against it, so a stale one would put
    /// every block in the wrong place.
    pub client_rect: Option<PixelRect>,
    pub slots: Vec<Slot>,
    /// How the last swap went.
    pub last_swap: Option<String>,
    /// When the pick cards were first seen, so the entrance animation can be waited out. Cleared the
    /// moment the cards are not up, so a second card phase waits again from scratch.
    pub cards_since: Option<Instant>,
    /// The last read of the champ-select session failed, so what is on screen is the previous one.
    ///
    /// Shown rather than acted on. The blocks stay where they are: a champ select that the client
    /// stopped answering about for a moment has not ended, and blanking the overlay on every dropped
    /// read is what made it disappear when a champion was picked.
    pub stale: bool,
}

impl ChampSelectState {
    /// Champ select is open, on a queue whose screen we know how to read.
    pub fn active(&self) -> bool {
        self.session.is_some()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Champions on the big cards right now: the subset during `BAN_PICK` once the cards have
    /// finished sliding in, nothing before that and nothing afterwards.
    pub fn offered(&self) -> &[i64] {
        match &self.session {
            Some(s) if s.cards_up() && self.cards_settled() => &s.subset,
            _ => &[],
        }
    }

    /// Have the cards been up long enough to have stopped moving? See [`CARDS_ENTRANCE`].
    pub fn cards_settled(&self) -> bool {
        self.cards_since.is_some_and(|since| since.elapsed() >= CARDS_ENTRANCE)
    }

    /// Rebuilds every block from the layout and the session.
    ///
    /// Deterministic in the session and the champion table: the same pair always produces the same
    /// blocks. That is what makes the overlay stable — there is no sampled input that can disagree
    /// with itself between two ticks. The table only changes when a new patch lands, so in practice
    /// the session is the only thing moving.
    pub fn rebuild(&mut self, layout: &ChampSelectLayout, rankings: &super::rankings::Rankings) {
        let (Some(client), Some(cards_up)) = (self.client_rect, self.session.as_ref().map(ChampSelect::cards_up))
        else {
            self.slots.clear();
            return;
        };

        // Start (or reset) the entrance clock before anything reads it. `cards_up()` is the phase and
        // the offer only, so this flips exactly when the cards appear and disappear.
        if cards_up {
            self.cards_since.get_or_insert_with(Instant::now);
        } else {
            self.cards_since = None;
        }
        let Some(session) = &self.session else { return };

        let mut slots = Vec::new();
        let block = |surface, index, rect, champion_id, swappable| Slot {
            surface,
            index,
            rect,
            champion_id,
            stats: stats_for(rankings, champion_id),
            swappable,
        };

        // The cards, while they are up. The buttons are not here: the cards vanish after about
        // fifteen seconds, and a button that disappears while being aimed at is worse than none.
        let offered = self.offered();
        for (index, rect) in layout.pick_card_blocks(&client, offered.len()).into_iter().enumerate() {
            slots.push(block(Surface::PickCard, index, rect, offered[index], false));
        }

        // The available-champions strip is the bench, in order, and it is what a swap targets.
        for (index, rect) in layout.strip_blocks(&client, session.bench.len()).into_iter().enumerate() {
            slots.push(block(Surface::Strip, index, rect, session.bench[index], true));
        }

        // One row per seat. A seat with nobody in it gets no block, and — crucially — does not shift
        // the rows below it: `team` is indexed by seat, holes included.
        for (index, rect) in layout.ally_blocks(&client).into_iter().enumerate() {
            let Some(champion_id) = session.team.get(index).copied().flatten() else { continue };
            slots.push(block(Surface::Ally, index, rect, champion_id, false));
        }
        self.slots = slots;
    }

    /// The buttons, in physical screen pixels.
    ///
    /// The overlay is click-through, so it never sees the pointer; whether it should accept a click
    /// has to be decided from outside the webview, against these rectangles. [`Slot::rect`] is
    /// already in screen pixels, so there is nothing to offset.
    pub fn clickable_screen_rects(&self) -> Vec<PixelRect> {
        self.slots.iter().filter(|s| s.swappable).map(|s| s.rect).collect()
    }
}

pub fn spawn(engine: Arc<Engine>) {
    super::spawn_task(&engine, "the champ-select layout", run(engine.clone()));
}

async fn run(engine: Arc<Engine>) {
    loop {
        tokio::time::sleep(POLL).await;

        let active = {
            let mut st = engine.lock();
            let active = st.champ_select.active();
            if !active && !st.champ_select.slots.is_empty() {
                st.champ_select.clear();
            }
            active
        };
        if !active {
            continue;
        }

        // Found outside the lock: it asks the window manager.
        let rect = client_area();
        let mut guard = engine.lock();
        // Reborrow once so the layout and the table can be read while the blocks are rebuilt.
        let st: &mut super::EngineState = &mut guard;
        st.champ_select.client_rect = rect;
        st.champ_select.rebuild(&st.tuning.champ_select_layout, &st.rankings);
    }
}

/// The client window's client area on the desktop.
///
/// Whether the window is in front no longer matters: nothing reads its pixels, so there is nothing
/// to be occluded. The overlay's own visibility is handled where it always was, in `overlay::sync`.
#[cfg(windows)]
fn client_area() -> Option<PixelRect> {
    let hwnd = mayhem_vision::windows::find_client_window()?;
    mayhem_vision::windows::client_rect_on_screen(hwnd).ok()
}

#[cfg(not(windows))]
fn client_area() -> Option<PixelRect> {
    None
}

#[cfg(test)]
mod tests {
    use league_api::lcu::{GameflowPhase, QueueInfo};

    use crate::engine::snapshot::ChampSelectSlotView;
    use crate::engine::{champselect, rankings, EngineState};

    fn queue(id: i64, description: &str) -> QueueInfo {
        QueueInfo { id, description: description.into(), game_mode: String::new(), map_id: 12 }
    }

    /// A champ select as the LCU reports it. `team` is by seat, so `None` is a player with no
    /// champion yet and the hole must survive into the rows.
    fn session(team: Vec<Option<i64>>, bench: Vec<i64>, subset: Vec<i64>, phase: &str) -> league_api::lcu::ChampSelect {
        league_api::lcu::ChampSelect {
            local_champion: team.iter().flatten().next().copied(),
            team,
            bench,
            subset,
            phase: phase.into(),
            time_left_ms: 8706,
            phase_total_ms: 15_000,
        }
    }

    /// A champion table for the tests, covering every id the champ-select tests use.
    ///
    /// Deliberately **not** exhaustive: `UNRANKED` is left out so the missing-row case can be
    /// exercised against the same helper rather than a special one.
    const UNRANKED: i64 = 9001;

    fn test_rankings() -> rankings::Rankings {
        let ids: [i64; 15] = [126, 113, 17, 103, 235, 360, 112, 40, 421, 92, 121, 114, 58, 157, 104];
        let by_id = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                (
                    id,
                    aramkit_client::ChampionRanking {
                        id,
                        rank: i as i64 + 1,
                        tier: Some("A".into()),
                        // Fractions, as the service reports them.
                        win_rate: 0.52 + i as f64 / 1000.0,
                        pick_rate: 0.03,
                        sample_count: 500_000,
                    },
                )
            })
            .collect();
        rankings::Rankings {
            by_id,
            pool_size: 173,
            patch: "16.19".into(),
            data_date: "2026-09-29".into(),
            freshness: Some(aramkit_client::Freshness::Live),
            error: None,
        }
    }

    /// Rebuilds the blocks the way the poll loop does, from the layout and the champion table.
    fn rebuild(st: &mut EngineState) {
        st.champ_select.rebuild(&mayhem_core::champselect::ChampSelectLayout::default(), &st.rankings);
    }

    fn champ_select_state(cs: league_api::lcu::ChampSelect) -> EngineState {
        use mayhem_core::geometry::PixelRect;
        let mut st = EngineState::default();
        st.client.queue = Some(queue(2400, "ARAM: Mayhem"));
        st.client.phase = Some(GameflowPhase::ChampSelect);
        st.champ_select.session = Some(cs);
        st.champ_select.client_rect = Some(PixelRect::new(0, 0, 1920, 1080));
        // The cards have been up long enough to have settled; the wait itself is tested separately.
        st.champ_select.cards_since = backdated(5);
        st.rankings = test_rankings();
        rebuild(&mut st);
        st
    }

    /// An `Instant` that many seconds in the past, for the card entrance clock.
    fn backdated(secs: u64) -> Option<std::time::Instant> {
        let now = std::time::Instant::now();
        Some(now.checked_sub(std::time::Duration::from_secs(secs)).unwrap_or(now))
    }

    fn blocks(st: &EngineState, surface: champselect::Surface) -> Vec<ChampSelectSlotView> {
        st.snapshot().champ_select.slots.into_iter().filter(|s| s.surface == surface).collect()
    }

    /// The row a teammate's block sits on is their **seat**, not their position in a filtered list.
    ///
    /// From a real dump, `myTeam` champions were `[126, 0, 113, 17, 0]`. Compacting that away would
    /// draw 113's numbers on row 2 — a teammate labelled with someone else's statistics.
    #[test]
    fn an_empty_seat_does_not_shift_the_rows_below_it() {
        let st = champ_select_state(session(
            vec![Some(126), None, Some(113), Some(17), None],
            Vec::new(),
            Vec::new(),
            "BAN_PICK",
        ));
        let allies = blocks(&st, champselect::Surface::Ally);
        assert_eq!(allies.len(), 3, "only the seats that have someone in them");
        assert_eq!(
            allies.iter().map(|a| (a.index, a.champion_id)).collect::<Vec<_>>(),
            vec![(0, 126), (2, 113), (3, 17)],
            "each block keeps its seat's index"
        );

        // And those indices are rows, one pitch apart, in order.
        let pitch = allies[2].y - allies[1].y;
        assert!((allies[1].y - allies[0].y - 2.0 * pitch).abs() < 1e-4, "row 3 is two pitches below row 1");
        assert!(allies.iter().all(|a| !a.swappable));
    }

    /// Whether the big cards are drawn comes from the phase the client reports, not from the screen.
    #[test]
    fn the_cards_follow_the_phase_and_show_the_subset() {
        // The offer, during its fifteen-second window. Ids from a real `BAN_PICK` dump, where the
        // bench held something else entirely.
        let mut st = champ_select_state(session(vec![None; 5], vec![103, 235, 360], vec![112, 40, 421], "BAN_PICK"));
        let cards = blocks(&st, champselect::Surface::PickCard);
        assert_eq!(cards.iter().map(|c| c.champion_id).collect::<Vec<_>>(), vec![112, 40, 421]);
        assert!(cards.iter().all(|c| !c.swappable), "the buttons live on the strip");
        assert!((cards[1].x + cards[1].width / 2.0 - 0.5).abs() < 0.002, "the middle card is centred");
        assert_eq!(st.snapshot().champ_select.card_count, 3);

        // The cards are left alone while they slide in, then stay up for the rest of the phase.
        st.champ_select.cards_since = None;
        rebuild(&mut st);
        assert!(blocks(&st, champselect::Surface::PickCard).is_empty(), "wait, they are still moving");
        st.champ_select.cards_since = backdated(3);
        rebuild(&mut st);
        assert_eq!(blocks(&st, champselect::Surface::PickCard).len(), 3, "settled after three seconds");
        // Late in the phase, with barely any time left, they are still drawn.
        st.champ_select.session.as_mut().unwrap().time_left_ms = 200;
        rebuild(&mut st);
        assert_eq!(blocks(&st, champselect::Surface::PickCard).len(), 3, "the phase timer is not the gate");

        // By finalisation the cards are gone, even though the subset endpoint still answers.
        st.champ_select.session.as_mut().unwrap().phase = "FINALIZATION".into();
        rebuild(&mut st);
        assert!(blocks(&st, champselect::Surface::PickCard).is_empty());
        assert_eq!(st.snapshot().champ_select.card_count, 0);

        // A two-champion offer is the same layout, one card narrower.
        let two = champ_select_state(session(vec![None; 5], Vec::new(), vec![112, 40], "BAN_PICK"));
        let two = blocks(&two, champselect::Surface::PickCard);
        assert_eq!(two.len(), 2);
        assert!(two[0].x > cards[0].x, "two cards start further in than three");
        assert!((two[0].width - cards[0].width).abs() < 1e-6, "the cards keep their size");
    }

    /// The strip is the bench, and it is the only surface a swap can target.
    #[test]
    fn strip_blocks_are_the_bench_and_carry_the_buttons() {
        let st =
            champ_select_state(session(vec![Some(799)], vec![92, 121, 114, 58, 157, 104], Vec::new(), "FINALIZATION"));
        let strip = blocks(&st, champselect::Surface::Strip);
        assert_eq!(strip.iter().map(|s| s.champion_id).collect::<Vec<_>>(), vec![92, 121, 114, 58, 157, 104]);
        assert!(strip.iter().all(|s| s.swappable));
        assert_eq!(st.champ_select.clickable_screen_rects().len(), 6);
        assert_eq!(st.snapshot().champ_select.bench_size, 6);

        // Evenly pitched, left to right.
        let pitch = strip[1].x - strip[0].x;
        assert!(strip.windows(2).all(|w| (w[1].x - w[0].x - pitch).abs() < 1e-4));

        // An empty bench is simply no blocks and no buttons.
        let empty = champ_select_state(session(vec![Some(799)], Vec::new(), Vec::new(), "FINALIZATION"));
        assert!(blocks(&empty, champselect::Surface::Strip).is_empty());
        assert!(empty.champ_select.clickable_screen_rects().is_empty());
    }

    /// Buttons are published in physical screen pixels, because the pointer is tracked from outside
    /// the webview and a rectangle relative to the client area would be tracked in the wrong place.
    #[test]
    fn button_rectangles_are_offset_by_the_windows_position_on_the_desktop() {
        use mayhem_core::geometry::PixelRect;

        let mut st = champ_select_state(session(vec![Some(799)], vec![92, 121], Vec::new(), "FINALIZATION"));
        let at_origin = st.champ_select.clickable_screen_rects();
        assert_eq!(at_origin.len(), 2);

        st.champ_select.client_rect = Some(PixelRect::new(300, 120, 1920, 1080));
        rebuild(&mut st);
        for (a, b) in at_origin.iter().zip(&st.champ_select.clickable_screen_rects()) {
            assert_eq!((b.x - a.x, b.y - a.y), (300, 120));
        }

        // Champ select over: no buttons, so the overlay goes back to letting every click through.
        st.champ_select.clear();
        assert!(st.champ_select.clickable_screen_rects().is_empty());
    }

    /// The figures on a block are the real aramkit numbers for that champion, and the units are
    /// converted exactly once.
    ///
    /// The service reports rates as fractions (`0.52`) because aramkit does; a block shows a
    /// percentage (`52.0`). Converting in two places, or in none, is the obvious way for this to go
    /// wrong, and a win rate of `0.52%` or `5200%` would still render as a perfectly plausible
    /// block — so the arithmetic is pinned rather than eyeballed.
    #[test]
    fn a_block_carries_that_champions_real_numbers_as_percentages() {
        let st = champ_select_state(session(vec![Some(126)], vec![103], vec![112, 40], "BAN_PICK"));

        let ally = &blocks(&st, champselect::Surface::Ally)[0];
        let stats = ally.stats.as_ref().expect("126 is in the table");
        assert_eq!(ally.champion_id, 126);
        assert_eq!(stats.rank, 1, "aramkit's rank, passed through");
        assert_eq!(stats.pool_size, 173, "what the rank is out of, from the service");
        assert_eq!(stats.tier.as_deref(), Some("A"));
        // 0.52 upstream is 52%, not 0.52% and not 5200%.
        assert!((stats.win_rate - 52.0).abs() < 1e-3, "win rate was {}", stats.win_rate);
        assert!((stats.pick_rate - 3.0).abs() < 1e-3, "pick rate was {}", stats.pick_rate);
        assert_eq!(stats.sample_count, 500_000);

        // Two champions on screen do not share a row.
        let card = blocks(&st, champselect::Surface::PickCard);
        let ranks: Vec<_> = card.iter().filter_map(|c| c.stats.as_ref().map(|s| s.rank)).collect();
        assert_eq!(ranks.len(), 2);
        assert_ne!(ranks[0], ranks[1], "each block is looked up by its own champion");
    }

    /// A champion the table has no row for gets a block with **no figures**, not invented ones.
    ///
    /// This is a real case, not a defensive one: a champion released this patch has no row, and
    /// *every* champion has none while the table is still being fetched or when the service has
    /// never been reachable. The block must still be drawn, because on the bench it is the swap
    /// button and dropping it would drop the ability to swap.
    #[test]
    fn a_champion_the_table_does_not_rank_gets_a_block_without_figures() {
        let st = champ_select_state(session(vec![Some(126)], vec![UNRANKED], Vec::new(), "FINALIZATION"));

        let strip = blocks(&st, champselect::Surface::Strip);
        assert_eq!(strip.len(), 1, "the block is still there");
        assert_eq!(strip[0].champion_id, UNRANKED);
        assert!(strip[0].stats.is_none(), "no numbers rather than plausible ones");
        assert!(strip[0].swappable, "and it is still the swap button");
        assert_eq!(st.champ_select.clickable_screen_rects().len(), 1);

        // The champion who *is* ranked on the same screen is unaffected.
        assert!(blocks(&st, champselect::Surface::Ally)[0].stats.is_some());
    }

    /// With no table at all the blocks are still laid out, and the snapshot says why they are bare.
    ///
    /// The overlay used to warn that its numbers were invented. Now that they are real it reports
    /// the patch instead, and this is the case where it has to admit it has nothing.
    #[test]
    fn without_a_champion_table_the_blocks_are_bare_and_the_snapshot_says_so() {
        let mut st = champ_select_state(session(vec![Some(126)], vec![103, 235], Vec::new(), "FINALIZATION"));
        st.rankings = rankings::Rankings { error: Some("service unreachable".into()), ..Default::default() };
        rebuild(&mut st);

        let view = st.snapshot().champ_select;
        assert_eq!(view.slots.len(), 3, "position does not depend on the statistics");
        assert!(view.slots.iter().all(|s| s.stats.is_none()));
        assert_eq!(view.ranked_champions, 0);
        assert!(view.data_patch.is_empty(), "no patch to claim");
        assert_eq!(view.rankings_error.as_deref(), Some("service unreachable"));
    }

    /// A rank is only meaningful against what it is out of, so a table that lost its `poolSize`
    /// shows no figures rather than a bare `# 3`.
    #[test]
    fn a_rank_with_no_pool_size_is_not_shown() {
        let mut st = champ_select_state(session(vec![Some(126)], Vec::new(), Vec::new(), "FINALIZATION"));
        assert!(blocks(&st, champselect::Surface::Ally)[0].stats.is_some());

        st.rankings.pool_size = 0;
        rebuild(&mut st);
        assert!(blocks(&st, champselect::Surface::Ally)[0].stats.is_none());
    }

    /// Where the numbers came from travels with them, so the overlay can mark an offline copy.
    #[test]
    fn the_snapshot_reports_the_patch_the_numbers_are_for() {
        let mut st = champ_select_state(session(vec![Some(126)], Vec::new(), Vec::new(), "FINALIZATION"));
        let view = st.snapshot().champ_select;
        assert_eq!(view.data_patch, "16.19");
        assert_eq!(view.data_date, "2026-09-29");
        assert_eq!(view.freshness, Some(aramkit_client::Freshness::Live));
        assert_eq!(view.ranked_champions, 15);
        assert!(view.rankings_error.is_none());

        st.rankings.freshness = Some(aramkit_client::Freshness::Stale);
        assert_eq!(st.snapshot().champ_select.freshness, Some(aramkit_client::Freshness::Stale));
    }

    /// Without the client area there is nothing to normalise against, and a block placed against a
    /// guess is worse than no block.
    #[test]
    fn no_client_area_means_no_blocks() {
        let mut st = champ_select_state(session(vec![Some(126)], vec![92], Vec::new(), "FINALIZATION"));
        assert!(!st.snapshot().champ_select.slots.is_empty());

        st.champ_select.client_rect = None;
        rebuild(&mut st);
        let view = st.snapshot().champ_select;
        assert!(view.active, "champ select is still open");
        assert!(view.slots.is_empty());
    }

    /// The same session always produces the same blocks. This is the property that stopped the
    /// overlay flickering: there is no sampled input that can disagree with itself between ticks.
    #[test]
    fn rebuilding_from_the_same_session_is_stable() {
        let mut st = champ_select_state(session(
            vec![Some(126), None, Some(113)],
            vec![103, 235],
            vec![112, 40, 421],
            "BAN_PICK",
        ));
        let first = st.snapshot().champ_select.slots;
        for _ in 0..5 {
            rebuild(&mut st);
        }
        let again = st.snapshot().champ_select.slots;
        assert_eq!(first.len(), again.len());
        assert!(first
            .iter()
            .zip(&again)
            .all(|(a, b)| a.champion_id == b.champion_id && a.x == b.x && a.y == b.y && a.surface == b.surface));
    }
}
