//! Where the ARAM: Mayhem champ-select furniture is, fitted from real screenshots.
//!
//! The screen has three surfaces worth labelling:
//!
//! - **the pick cards** — the champions being chosen between, centred, **two or three of them**;
//! - **the available-champions strip** — square portraits along the top, in a fixed row of ten;
//! - **the ally column** — five circular portraits down the left.
//!
//! Every group is evenly pitched, but **only the pick cards centre on their own contents**: three
//! cards are not a different layout from two, just a wider one, which matters because Riot varies the
//! number of them. The strip does the opposite — its ten boxes are a fixed row and the bench fills
//! them from the left, so a short bench leaves empty frames on the right rather than pulling in. That
//! difference is easy to miss and expensive to get wrong: centring the strip by bench size puts every
//! block over the wrong champion (see [`STRIP_SLOTS`]).
//!
//! # The coordinate model
//!
//! The client draws a **16:9 user interface scaled to fit its window**, so positions are stored as
//! fractions of that box rather than of the window, and [`ChampSelectLayout::ui_box`] maps one to the
//! other. On a 16:9 window the two are identical, which is the only case measured so far — see
//! `[unverified]` below.
//!
//! # Provenance
//!
//! Measured off three 1920x1080 captures of a real ARAM: Mayhem champ select by scanning luminance
//! profiles for the borders (`crates/mayhem-vision/tests/champ_select_measure.rs`):
//!
//! | Thing | Measured at 1920x1080 |
//! |---|---|
//! | Pick card | 267 x 442, tops at y=270, gap 57, group centred on x=960 |
//! | Strip slot | 77 x 77, top at y=14, pitch 88, the *ten-slot row* centred on x=960 (x=527..1393) |
//! | Ally portrait | 100 x 100, left at x=76, first top at y=152, pitch 120 |
//!
//! `[unverified]`: only one window size has been seen, so the 16:9 model is a well-motivated
//! assumption rather than a measurement, and the three-card group has not been captured — its
//! geometry follows from the group being centred, which two cards do confirm.

use serde::{Deserialize, Serialize};

use crate::geometry::PixelRect;

/// The number of ally rows. Five a side, always.
pub const ALLY_ROWS: usize = 5;

/// The number of slots the available-champions strip draws, occupied or not.
///
/// **The row is a fixed ten boxes and the bench fills it from the left.** This is the one group on
/// the screen that is *not* sized to its contents: a bench of six leaves four empty frames on the
/// right rather than pulling the row in. Measured on two captures with different bench sizes — seven
/// filled and six filled — and the ten boxes span x=527..1393 at 1920x1080 in both, centred on 960.
pub const STRIP_SLOTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampSelectLayout {
    /// Pick card width, as a fraction of the interface width.
    pub card_width: f32,
    /// Pick card height, as a fraction of the interface height.
    pub card_height: f32,
    /// Top edge of the pick cards, as a fraction of the interface height.
    pub card_top: f32,
    /// Gap between adjacent pick cards, as a fraction of the interface width.
    pub card_gap: f32,

    /// Side of one available-champions slot, as a fraction of the interface height.
    pub strip_slot: f32,
    /// Gap between adjacent slots, as a fraction of the interface width.
    pub strip_gap: f32,
    /// Top edge of the strip, as a fraction of the interface height.
    pub strip_top: f32,

    /// Left edge of the ally portraits, as a fraction of the interface width.
    pub ally_left: f32,
    /// Side of an ally portrait, as a fraction of the interface height.
    pub ally_side: f32,
    /// Top edge of the first ally portrait, as a fraction of the interface height.
    pub ally_top: f32,
    /// Distance between ally rows, as a fraction of the interface height.
    pub ally_pitch: f32,

    // --- Where the statistics blocks go. Each is placed against the thing it describes, so that
    // --- moving a surface moves its block with it.
    /// Inset of a card's block from the card's sides, as a fraction of the card's width.
    pub card_block_inset: f32,
    /// Top of a card's block, as a fraction of the card's height from its top.
    pub card_block_top: f32,
    /// Height of a card's block, as a fraction of the card's height.
    pub card_block_height: f32,
    /// Gap between a strip slot and the block beneath it, as a fraction of the interface height.
    ///
    /// **Zero on purpose.** The tier symbol straddles the block's top edge, half above and half
    /// below, so that edge has to be the boundary with the champion portrait itself. With the 10 px
    /// gap this used to have, the symbol's upper half hung in the gap instead of over the portrait,
    /// and the badge read as sitting *inside* the block rather than across the join.
    pub strip_block_gap: f32,
    /// Height of a strip slot's block, as a fraction of the interface height. Taller than the slot
    /// needs, because the tier symbol sits astride its top edge rather than inside it.
    pub strip_block_height: f32,
    /// Left edge of an ally's block, as a fraction of the interface width. It sits to the right of
    /// the champion and summoner names rather than over the portrait, which is already crowded.
    pub ally_block_left: f32,
    /// Width of an ally's block, as a fraction of the interface width.
    pub ally_block_width: f32,
    /// Height of an ally's block, as a fraction of the interface height. Centred on the portrait.
    pub ally_block_height: f32,
}

impl Default for ChampSelectLayout {
    fn default() -> Self {
        Self {
            card_width: 267.0 / 1920.0,
            card_height: 442.0 / 1080.0,
            card_top: 270.0 / 1080.0,
            card_gap: 57.0 / 1920.0,

            strip_slot: 77.0 / 1080.0,
            strip_gap: 11.0 / 1920.0,
            strip_top: 14.0 / 1080.0,

            ally_left: 76.0 / 1920.0,
            ally_side: 100.0 / 1080.0,
            ally_top: 152.0 / 1080.0,
            ally_pitch: 120.0 / 1080.0,

            card_block_inset: 10.0 / 267.0,
            card_block_top: 12.0 / 442.0,
            card_block_height: 78.0 / 442.0,
            strip_block_gap: 0.0,
            strip_block_height: 62.0 / 1080.0,
            ally_block_left: 352.0 / 1920.0,
            ally_block_width: 196.0 / 1920.0,
            ally_block_height: 64.0 / 1080.0,
        }
    }
}

impl ChampSelectLayout {
    /// The 16:9 interface box inside `client`, centred.
    ///
    /// A window that is not 16:9 has the interface letterboxed inside it rather than stretched, so
    /// everything is measured against this box and not against the window.
    pub fn ui_box(&self, client: &PixelRect) -> PixelRect {
        let (cw, ch) = (client.width as f32, client.height as f32);
        if cw <= 0.0 || ch <= 0.0 {
            return *client;
        }
        let (w, h) = if cw / ch > 16.0 / 9.0 {
            (ch * 16.0 / 9.0, ch) // window is wider than 16:9: bars left and right
        } else {
            (cw, cw * 9.0 / 16.0) // taller: bars top and bottom
        };
        PixelRect::new(
            client.x + ((cw - w) / 2.0).round() as i32,
            client.y + ((ch - h) / 2.0).round() as i32,
            w.round().max(1.0) as u32,
            h.round().max(1.0) as u32,
        )
    }

    /// The pick cards, left to right. `count` is however many the client is showing — **two or
    /// three**, and nothing here assumes which.
    pub fn pick_cards(&self, client: &PixelRect, count: usize) -> Vec<PixelRect> {
        let ui = self.ui_box(client);
        let w = self.card_width * ui.width as f32;
        let gap = self.card_gap * ui.width as f32;
        centred_row(&ui, count, w, gap, self.card_top * ui.height as f32, self.card_height * ui.height as f32)
    }

    /// The first `count` available-champions slots, left to right.
    ///
    /// **The whole row of [`STRIP_SLOTS`] is laid out and then truncated**, rather than centring
    /// `count` boxes. The two are only the same when the bench is full, and they diverge by a whole
    /// slot for every two champions missing: centring six boxes put every block two slots to the
    /// right of the champion it described, which is not a near miss but a wrong label.
    ///
    /// This is the one group that does not centre on its own contents — unlike the pick cards, where
    /// centring by count is exactly right.
    pub fn strip_slots(&self, client: &PixelRect, count: usize) -> Vec<PixelRect> {
        let ui = self.ui_box(client);
        let side = self.strip_slot * ui.height as f32;
        let gap = self.strip_gap * ui.width as f32;
        let mut row = centred_row(&ui, STRIP_SLOTS, side, gap, self.strip_top * ui.height as f32, side);
        row.truncate(count);
        row
    }

    /// The statistics block for each pick card: inside the card, near the top, clear of the champion
    /// name the client prints across the bottom.
    pub fn pick_card_blocks(&self, client: &PixelRect, count: usize) -> Vec<PixelRect> {
        self.pick_cards(client, count)
            .into_iter()
            .map(|c| {
                let inset = (c.width as f32 * self.card_block_inset).round() as i32;
                PixelRect::new(
                    c.x + inset,
                    c.y + (c.height as f32 * self.card_block_top).round() as i32,
                    (c.width as i32 - inset * 2).max(1) as u32,
                    (c.height as f32 * self.card_block_height).round().max(1.0) as u32,
                )
            })
            .collect()
    }

    /// The statistics block under each available-champions slot.
    pub fn strip_blocks(&self, client: &PixelRect, count: usize) -> Vec<PixelRect> {
        let ui = self.ui_box(client);
        let gap = (self.strip_block_gap * ui.height as f32).round() as i32;
        let height = (self.strip_block_height * ui.height as f32).round().max(1.0) as u32;
        self.strip_slots(client, count)
            .into_iter()
            .map(|s| PixelRect::new(s.x, s.y + s.height as i32 + gap, s.width, height))
            .collect()
    }

    /// The statistics block for each ally row, to the right of the champion and summoner names.
    pub fn ally_blocks(&self, client: &PixelRect) -> [PixelRect; ALLY_ROWS] {
        let ui = self.ui_box(client);
        let x = ui.x as f32 + self.ally_block_left * ui.width as f32;
        let width = (self.ally_block_width * ui.width as f32).round().max(1.0) as u32;
        let height = (self.ally_block_height * ui.height as f32).round().max(1.0) as u32;
        self.ally_portraits(client).map(|p| {
            // Centred on the portrait, so the two read as one row however their heights differ.
            let centre = p.y + p.height as i32 / 2;
            PixelRect::new(x.round() as i32, centre - height as i32 / 2, width, height)
        })
    }

    /// The five ally portraits, top to bottom.
    pub fn ally_portraits(&self, client: &PixelRect) -> [PixelRect; ALLY_ROWS] {
        let ui = self.ui_box(client);
        let side = self.ally_side * ui.height as f32;
        let x = ui.x as f32 + self.ally_left * ui.width as f32;
        std::array::from_fn(|i| {
            let y = ui.y as f32 + (self.ally_top + self.ally_pitch * i as f32) * ui.height as f32;
            PixelRect::new(
                x.round() as i32,
                y.round() as i32,
                side.round().max(1.0) as u32,
                side.round().max(1.0) as u32,
            )
        })
    }
}

/// `count` boxes of `item_width` separated by `gap`, centred horizontally in `ui`.
///
/// Centring is the whole point: it is what makes three pick cards the same layout as two, and it is
/// the one thing the screenshots confirm directly — two cards straddle the centre exactly.
fn centred_row(ui: &PixelRect, count: usize, item_width: f32, gap: f32, top: f32, height: f32) -> Vec<PixelRect> {
    if count == 0 || item_width <= 0.0 {
        return Vec::new();
    }
    let total = item_width * count as f32 + gap * (count.saturating_sub(1)) as f32;
    let x0 = ui.x as f32 + (ui.width as f32 - total) / 2.0;
    let y = ui.y as f32 + top;
    (0..count)
        .map(|i| {
            let x = x0 + (item_width + gap) * i as f32;
            PixelRect::new(
                x.round() as i32,
                y.round() as i32,
                item_width.round().max(1.0) as u32,
                height.round().max(1.0) as u32,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FHD: PixelRect = PixelRect::new(0, 0, 1920, 1080);

    /// Against the numbers measured off the real screenshots. Two pixels of slack for the rounding
    /// that turning a border line into a single coordinate involves.
    fn near(got: i32, want: i32, what: &str) {
        assert!((got - want).abs() <= 2, "{what}: {got} is not within 2 of the measured {want}");
    }

    #[test]
    fn two_pick_cards_land_where_they_were_measured() {
        let cards = ChampSelectLayout::default().pick_cards(&FHD, 2);
        assert_eq!(cards.len(), 2);
        near(cards[0].x, 663, "left card left edge");
        near(cards[0].x + cards[0].width as i32, 930, "left card right edge");
        near(cards[1].x, 987, "right card left edge");
        near(cards[1].x + cards[1].width as i32, 1254, "right card right edge");
        near(cards[0].y, 270, "card top");
        near(cards[0].y + cards[0].height as i32, 712, "card bottom");
    }

    /// Against a real three-card capture, whose card borders measure 501/768, 825/1092 and
    /// 1149/1416. This was predicted by the centred-group model before the capture existed, and the
    /// prediction landed within a pixel — so the two-or-three behaviour is measured now, not inferred.
    #[test]
    fn three_pick_cards_match_a_real_three_card_capture() {
        let cards = ChampSelectLayout::default().pick_cards(&FHD, 3);
        assert_eq!(cards.len(), 3);
        for (i, (left, right)) in [(501, 768), (825, 1092), (1149, 1416)].into_iter().enumerate() {
            near(cards[i].x, left, "card left edge");
            near(cards[i].x + cards[i].width as i32, right, "card right edge");
        }
    }

    /// Riot shows three cards sometimes. The group stays centred and the cards keep their size, so
    /// the row simply grows outwards by one pitch either side.
    #[test]
    fn three_pick_cards_stay_centred_and_keep_their_size() {
        let layout = ChampSelectLayout::default();
        let two = layout.pick_cards(&FHD, 2);
        let three = layout.pick_cards(&FHD, 3);
        assert_eq!(three.len(), 3);

        for c in &three {
            assert_eq!(c.width, two[0].width, "a third card does not shrink the others");
            assert_eq!(c.height, two[0].height);
            assert_eq!(c.y, two[0].y, "same row");
        }
        // Still centred on the interface, and evenly pitched.
        let centre = three[0].x + (three[2].x + three[2].width as i32 - three[0].x) / 2;
        near(centre, 960, "three-card group centre");
        let pitch = |v: &[PixelRect], i: usize| v[i + 1].x - v[i].x;
        assert_eq!(pitch(&three, 0), pitch(&three, 1), "evenly pitched");
        near(pitch(&three, 0), pitch(&two, 0), "same pitch as two cards");
        // The middle card of three sits where the centre of the screen is.
        near(three[1].x + three[1].width as i32 / 2, 960, "middle card is centred");
    }

    #[test]
    fn one_card_is_simply_centred_and_none_is_empty() {
        let layout = ChampSelectLayout::default();
        let one = layout.pick_cards(&FHD, 1);
        near(one[0].x + one[0].width as i32 / 2, 960, "a lone card is centred");
        assert!(layout.pick_cards(&FHD, 0).is_empty());
        assert!(layout.strip_slots(&FHD, 0).is_empty());
    }

    #[test]
    fn the_strip_matches_its_measured_pitch_and_slots() {
        let slots = ChampSelectLayout::default().strip_slots(&FHD, 10);
        assert_eq!(slots.len(), 10);
        near(slots[0].x, 525, "first slot");
        near(slots[9].x + slots[9].width as i32, 1393, "last slot right edge");
        near(slots[1].x - slots[0].x, 88, "strip pitch");
        near(slots[0].width as i32, 77, "slot side");
        near(slots[0].y, 14, "strip top");
        assert!(slots.iter().all(|s| s.width == s.height), "slots are square");
    }

    /// A part-full bench keeps the slots it has, rather than pulling the row into the middle.
    ///
    /// The regression this pins: `strip_slots` used to centre `count` boxes, so a bench of six put
    /// every block two slots right of its champion — the first block sat over the *third* portrait.
    /// The pick cards do centre by count, and the two behaviours are easy to confuse.
    #[test]
    fn a_partly_filled_bench_stays_left_aligned_in_the_fixed_row() {
        let layout = ChampSelectLayout::default();
        let full = layout.strip_slots(&FHD, STRIP_SLOTS);
        for count in 0..=STRIP_SLOTS {
            let some = layout.strip_slots(&FHD, count);
            assert_eq!(some.len(), count);
            assert_eq!(some, full[..count], "{count} on the bench moved the slots");
        }
        // Concretely: six champions occupy the left six boxes, not the middle six.
        let six = layout.strip_slots(&FHD, 6);
        near(six[0].x, 525, "a bench of six still starts at the left of the row");
        near(six[5].x, 525 + 5 * 88, "and its last slot is the sixth box, not the eighth");
        // The blocks follow their slots, which is the whole point of deriving one from the other.
        let blocks = layout.strip_blocks(&FHD, 6);
        for (slot, block) in six.iter().zip(&blocks) {
            assert_eq!(slot.x, block.x, "a block sits under its own slot");
        }
    }

    #[test]
    fn ally_portraits_match_their_measured_pitch() {
        let rows = ChampSelectLayout::default().ally_portraits(&FHD);
        near(rows[0].x, 76, "ally column left");
        near(rows[0].width as i32, 100, "ally portrait side");
        near(rows[0].y, 152, "first ally row");
        near(rows[1].y - rows[0].y, 120, "ally row pitch");
        near(rows[4].y, 152 + 4 * 120, "last ally row");
        assert!(rows.iter().all(|r| r.x == rows[0].x), "one column");
    }

    /// Each block belongs to the thing it describes: inside its card, under its strip slot, beside
    /// its ally row. Nothing is positioned independently, so a surface moving takes its block along.
    #[test]
    fn statistics_blocks_sit_against_the_thing_they_describe() {
        let layout = ChampSelectLayout::default();

        let cards = layout.pick_cards(&FHD, 3);
        let card_blocks = layout.pick_card_blocks(&FHD, 3);
        assert_eq!(card_blocks.len(), 3);
        for (card, block) in cards.iter().zip(&card_blocks) {
            assert!(block.x > card.x && block.x + block.width as i32 <= card.x + card.width as i32);
            assert!(block.y > card.y, "inside the card");
            // Clear of the champion name the client prints low on the card.
            assert!(block.y + block.height as i32 <= card.y + card.height as i32 / 2);
        }

        let slots = layout.strip_slots(&FHD, 10);
        let strip_blocks = layout.strip_blocks(&FHD, 10);
        for (slot, block) in slots.iter().zip(&strip_blocks) {
            assert_eq!((block.x, block.width), (slot.x, slot.width), "same column as its slot");
            assert!(block.y >= slot.y + slot.height as i32, "underneath it");
        }

        let portraits = layout.ally_portraits(&FHD);
        let ally_blocks = layout.ally_blocks(&FHD);
        for (portrait, block) in portraits.iter().zip(&ally_blocks) {
            assert!(block.x > portrait.x + portrait.width as i32, "to the right of the portrait");
            let (pc, bc) = (portrait.y + portrait.height as i32 / 2, block.y + block.height as i32 / 2);
            assert!((pc - bc).abs() <= 1, "vertically centred on its row");
        }
        // Clear of the names, which the client prints immediately right of the portrait, and wide
        // enough for the three parts of a block: win rate, tier symbol, pick rate.
        near(ally_blocks[0].x, 352, "ally block left");
        assert!(ally_blocks[0].width >= 170, "room for win rate, tier and pick rate side by side");
    }

    /// The interface is 16:9 and letterboxed, not stretched. A 16:9 window is the box itself; a
    /// wider one gets bars left and right, and everything stays put relative to the box.
    #[test]
    fn the_interface_box_is_sixteen_by_nine_inside_any_window() {
        let layout = ChampSelectLayout::default();
        assert_eq!(layout.ui_box(&FHD), FHD, "a 16:9 window is the interface");

        let wide = PixelRect::new(0, 0, 2560, 1080); // 21:9
        let ui = layout.ui_box(&wide);
        assert_eq!((ui.width, ui.height), (1920, 1080));
        assert_eq!(ui.x, 320, "bars left and right");

        let tall = PixelRect::new(0, 0, 1280, 1024); // 5:4
        let ui = layout.ui_box(&tall);
        assert_eq!((ui.width, ui.height), (1280, 720));
        assert_eq!(ui.y, 152, "bars top and bottom");
    }

    /// Halving the window halves every measurement: the interface scales, it does not reflow.
    #[test]
    fn everything_scales_with_the_window() {
        let layout = ChampSelectLayout::default();
        let half = PixelRect::new(0, 0, 960, 540);
        let big = layout.pick_cards(&FHD, 2);
        let small = layout.pick_cards(&half, 2);
        near(small[0].x * 2, big[0].x, "card x scales");
        near(small[0].width as i32 * 2, big[0].width as i32, "card width scales");

        let rows = layout.ally_portraits(&half);
        near(rows[0].width as i32 * 2, 100, "ally side scales");
        near((rows[1].y - rows[0].y) * 2, 120, "ally pitch scales");
    }

    /// A window offset on the desktop moves everything with it and changes nothing else.
    #[test]
    fn a_window_offset_moves_everything_with_it() {
        let layout = ChampSelectLayout::default();
        let moved = PixelRect::new(300, 120, 1920, 1080);
        let a = layout.pick_cards(&FHD, 3);
        let b = layout.pick_cards(&moved, 3);
        for (a, b) in a.iter().zip(&b) {
            assert_eq!((b.x - a.x, b.y - a.y), (300, 120));
            assert_eq!((a.width, a.height), (b.width, b.height));
        }
    }
}
