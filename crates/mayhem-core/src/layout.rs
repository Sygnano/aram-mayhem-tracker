//! Where the augment cards are, on any resolution and aspect ratio.
//!
//! Measured from real screenshots at 1024×768, 1280×1024, 1600×900, 1920×1080 and 2000×844
//! (4:3, 5:4, 16:9, ~21:9) on 2026-09-29. The augment screen is **anchored at the horizontal centre
//! and scales with the window height only**: every position below is in units of the client
//! height `H`, horizontal ones measured from the centre. HUD scale does not affect it (checked in
//! game). The CommunityDragon card frame, scaled by `card_height`, matches all 15 measured cards
//! with a mean normalised correlation of 0.973.

use serde::{Deserialize, Serialize};

use crate::augments::OFFER_SLOTS;
use crate::geometry::PixelRect;

/// Card geometry in units of the client height. Configurable so a future patch that moves the
/// cards is a config change, not a code change.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AugmentLayout {
    /// Card centres, left to right, relative to the client's horizontal centre.
    pub card_centers: [f32; OFFER_SLOTS],
    /// Top of the card frame.
    pub card_top: f32,
    pub card_width: f32,
    pub card_height: f32,
    /// Title band, relative to the client top, and its half-width around the card centre. This is
    /// the band of a one-line title, which is also the **bottom line** of a wrapped one.
    pub title_top: f32,
    pub title_height: f32,
    pub title_half_width: f32,
    /// A long title wraps, and the block grows **upwards**: the bottom line stays in the title
    /// band and each further line sits this far above the one below it.
    pub title_line_pitch: f32,
    /// How many lines a title is looked for on. Bands above the first are only read when there is
    /// text in them.
    pub title_max_lines: u32,
    /// Stat anvil cards only: the tops of the two value lines under the shard's name, and
    /// the height and half-width of each crop. A shard card is drawn in the augment card's frame
    /// at the augment card's position, so these are the only anvil-specific numbers.
    pub value_tops: [f32; 2],
    pub value_height: f32,
    pub value_half_width: f32,
    /// Where a stat anvil rank label's **bottom edge** goes: inside the card, just above the
    /// bottom frame, so the label sits wholly in the card rather than straddling its border.
    pub anvil_label_bottom: f32,
    /// Augment icon: centre height and side.
    pub icon_center_y: f32,
    pub icon_size: f32,
    /// Thickness of the frame's decorative border, in units of client height. Only used to place
    /// overlay panels on the border line rather than on the outer edge.
    pub frame_border: f32,
    /// Top edge of the reroll button under each card. Overlay panels must stay above this line.
    pub reroll_top: f32,
    /// The box the reroll button is *looked for* in, under each card centre: the plate plus a few
    /// pixels of margin. Distinct from `reroll_top`, which is a clearance line for the panels and
    /// marks the plate's own top border rather than the search box's.
    pub reroll_box_top: f32,
    pub reroll_box_width: f32,
    pub reroll_box_height: f32,
    /// The "hide augments" button: centre offset, top edge and size, in the same units as the
    /// cards. It is centred on the client and scales with height just as the cards do.
    pub button_center_x: f32,
    pub button_top: f32,
    pub button_width: f32,
    pub button_height: f32,
    /// Overlay panel width, as a fraction of the card frame's width.
    ///
    /// Configurable because it is a taste call, but it is not free: the panel's three groups have to
    /// fit side by side, and the width they need scales with the font size. At the current type
    /// sizes the content comes to about 25.3 units of client height against the 25.9 this gives, so
    /// shrinking it much wraps the per-level column onto its own line (seen in game at 0.5).
    pub panel_width: f32,
}

impl Default for AugmentLayout {
    fn default() -> Self {
        // Frame asset is 310×512 px, drawn at 1.0275× its size at 1080p: 0.2949H × 0.4871H.
        Self {
            card_centers: [-0.3346, 0.0062, 0.3470],
            card_top: 0.1793,
            card_width: 0.2949,
            card_height: 0.4871,
            // Title text spans 0.404–0.421H; the category tag starts at ~0.429H.
            title_top: 0.398,
            title_height: 0.029,
            title_half_width: 0.125,
            // Measured on "Expertise en omnivampirisme" at 1024x768, 1920x1200 and 3424x1401
            // (2026-10-01): line one spans 0.378-0.397H and line two 0.403-0.422H, while the
            // one-line titles beside it span 0.402-0.418H. So the last line does not move and the
            // pitch is 0.025H, within 0.001H at all three sizes.
            title_line_pitch: 0.025,
            // Two lines is all that has been seen. The longest names in the pool (49 characters
            // in German, against 27 for the measured two-liner) would need three, and the card is
            // empty down from the icon at 0.332H, so a third band at 0.348H costs nothing.
            title_max_lines: 3,
            // Measured on seven anvil screenshots at five resolutions (2026-10-01): line
            // one spans 0.4688-0.4838H and line two 0.5000-0.5152H, at most 0.005H apart between
            // screenshots; the widest line is 0.101H either side of the card centre. Each crop is
            // the line plus about a third of a line height above and below, as the title crop is.
            value_tops: [0.465, 0.497],
            value_height: 0.022,
            value_half_width: 0.110,
            // The inner edge of the bottom frame, measured at each card's centre on the seven anvil
            // screenshots, is at 0.6465-0.6486H. 0.0085H below that (9px at 1080p) leaves a small
            // gap between the label and the frame.
            anvil_label_bottom: 0.638,
            icon_center_y: 0.2875,
            icon_size: 0.14,
            // Measured on a 3440x1440 in-game capture (2026-09-30): the bright border band spans
            // y 933-961, so 28px at 1440p.
            frame_border: 0.0194,
            // Same capture: the reroll button's top border is at y 990.
            reroll_top: 0.6875,
            // The reroll search box, swept rather than eyeballed by
            // `mayhem-vision/tests/reroll_measure.rs` over every augment screenshot we have: the
            // template's footprint *is* this box, so its size decides the detector's margin. Too
            // wide and the template carries background, which differs between screenshots and
            // drowns out a dim spent plate; too tight and the plate's border, its strongest
            // structure, is cut off. A first guess of 0.0715x0.0458 by eye scored *worse than
            // noise*; the swept optimum below separates the worst real button (0.773) from the best
            // false one anywhere on screen (0.452). 78x52 px at 1440p, centred at 0.7042H.
            reroll_box_top: 0.6862,
            reroll_box_width: 0.0540,
            reroll_box_height: 0.0360,
            // The "hide augments" button, measured on all seven augment screenshots by
            // `mayhem-vision/tests/augment_button_measure.rs` (2026-09-30). Its teal plate came out
            // at centre offset -0.0007..+0.0009, top 0.7701..0.7722, width 0.1767..0.1792 and
            // height 0.0422..0.0466 of the client height — a spread of two or three pixels at
            // 1080p, so the button is centre-anchored and height-scaled exactly like the cards.
            // Below is that plate grown by 0.0074H (8 px at 1080p) on every side, which is the
            // footprint of the bundled template: the gold outer border included.
            button_center_x: 0.0,
            button_top: 0.7637,
            button_width: 0.1926,
            button_height: 0.0593,
            panel_width: 0.88,
        }
    }
}

/// How a title block divides into lines, in units of the title band's own height so that it holds
/// for a manually calibrated title region as well as for the fitted layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TitleLines {
    /// Lines the block has room for.
    pub max: u32,
    /// Line pitch as a fraction of one line band's height.
    pub pitch: f32,
}

impl TitleLines {
    /// A block that is one band: nothing is looked for above it.
    pub const SINGLE: Self = Self { max: 1, pitch: 0.0 };

    /// How far above a title band of `band_height` pixels the block reaches.
    pub fn rise(&self, band_height: u32) -> u32 {
        ((self.max.max(1) - 1) as f32 * self.pitch * band_height as f32).round() as u32
    }

    /// The band of line `k` inside a block `block_height` pixels tall, as `(top, height)`. Line 0
    /// is the bottom line.
    pub fn band(&self, block_height: u32, k: u32) -> (u32, u32) {
        let band = block_height as f32 / (1.0 + (self.max.max(1) - 1) as f32 * self.pitch);
        let top = (block_height as f32 - band * (1.0 + k as f32 * self.pitch)).round().max(0.0) as u32;
        let top = top.min(block_height.saturating_sub(1));
        (top, (band.round() as u32).clamp(1, block_height - top))
    }
}

/// Clearance between an overlay panel and the reroll button, in units of client height (6px at
/// 1440p) - enough that the panel never touches the button at any resolution.
const PANEL_GAP: f32 = 0.004;

impl AugmentLayout {
    fn px(client: &PixelRect, cx: f32, top: f32, width: f32, height: f32) -> PixelRect {
        let h = client.height as f32;
        let centre = client.x as f32 + client.width as f32 / 2.0;
        let x = (centre + (cx - width / 2.0) * h).round() as i32;
        let y = (client.y as f32 + top * h).round() as i32;
        PixelRect::new(x, y, (width * h).round().max(1.0) as u32, (height * h).round().max(1.0) as u32)
    }

    /// The card frame (outer edge) of card `i`.
    pub fn card(&self, client: &PixelRect, i: usize) -> PixelRect {
        Self::px(client, self.card_centers[i], self.card_top, self.card_width, self.card_height)
    }

    /// The title text band of card `i`: a one-line title, or the bottom line of a wrapped one.
    pub fn title(&self, client: &PixelRect, i: usize) -> PixelRect {
        Self::px(client, self.card_centers[i], self.title_top, self.title_half_width * 2.0, self.title_height)
    }

    /// How a title block is stacked: what it takes to grow a title band into the block of a
    /// wrapped title, and to cut that block back into lines.
    pub fn title_lines(&self) -> TitleLines {
        TitleLines { max: self.title_max_lines.max(1), pitch: (self.title_line_pitch / self.title_height).max(0.0) }
    }

    /// Where card `i`'s reroll button is looked for. The rerolls share the cards' centres.
    pub fn reroll(&self, client: &PixelRect, i: usize) -> PixelRect {
        Self::px(client, self.card_centers[i], self.reroll_box_top, self.reroll_box_width, self.reroll_box_height)
    }

    /// The two value lines of stat anvil card `i`, top first.
    pub fn values(&self, client: &PixelRect, i: usize) -> [PixelRect; 2] {
        self.value_tops
            .map(|top| Self::px(client, self.card_centers[i], top, self.value_half_width * 2.0, self.value_height))
    }

    pub fn rerolls(&self, client: &PixelRect) -> [PixelRect; OFFER_SLOTS] {
        std::array::from_fn(|i| self.reroll(client, i))
    }

    /// The "hide augments" button, which is on screen exactly while augments can be selected.
    pub fn button(&self, client: &PixelRect) -> PixelRect {
        Self::px(client, self.button_center_x, self.button_top, self.button_width, self.button_height)
    }

    /// The augment icon of card `i`.
    pub fn icon(&self, client: &PixelRect, i: usize) -> PixelRect {
        let top = self.icon_center_y - self.icon_size / 2.0;
        Self::px(client, self.card_centers[i], top, self.icon_size, self.icon_size)
    }

    /// The y of the *centre of the bottom border* of card `i`, in physical pixels.
    ///
    /// Overlay panels are centred on this line, so they straddle the border the way Blitz's do,
    /// rather than sitting on the frame's outer edge.
    pub fn bottom_border_y(&self, client: &PixelRect, _i: usize) -> f32 {
        let h = client.height as f32;
        client.y as f32 + (self.card_top + self.card_height - self.frame_border / 2.0) * h
    }

    /// Where an overlay panel's **bottom edge** goes: just above the reroll button.
    ///
    /// Panels are anchored by their bottom and grow upwards, because anything that grows downwards
    /// covers the reroll button. At the height the panel currently has this still leaves it centred
    /// on the frame's bottom border, which is where it was placed to begin with.
    pub fn panel_bottom_y(&self, client: &PixelRect) -> f32 {
        let h = client.height as f32;
        client.y as f32 + (self.reroll_top - PANEL_GAP) * h
    }

    /// Where a stat anvil label's bottom edge goes, in physical pixels.
    pub fn anvil_label_bottom_y(&self, client: &PixelRect) -> f32 {
        client.y as f32 + self.anvil_label_bottom * client.height as f32
    }

    pub fn cards(&self, client: &PixelRect) -> [PixelRect; OFFER_SLOTS] {
        std::array::from_fn(|i| self.card(client, i))
    }

    pub fn titles(&self, client: &PixelRect) -> [PixelRect; OFFER_SLOTS] {
        std::array::from_fn(|i| self.title(client, i))
    }

    /// Whether all three cards fit inside the client area (a very narrow window might clip them).
    pub fn fits(&self, client: &PixelRect) -> bool {
        self.cards(client).iter().all(|c| c.intersect(client) == Some(*c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Measured against a real 3440x1440 capture (2026-09-30): the frame's bright bottom border
    /// spans y 933-961, centre 947, and the card's outer bottom edge reads 959.
    #[test]
    fn the_bottom_border_line_matches_a_real_capture() {
        let client = PixelRect::new(0, 0, 3440, 1440);
        let layout = AugmentLayout::default();

        let card = layout.card(&client, 0);
        let outer_bottom = card.y + card.height as i32;
        assert!((outer_bottom - 959).abs() <= 3, "outer bottom edge was {outer_bottom}");

        let border = layout.bottom_border_y(&client, 0);
        assert!((border - 947.0).abs() <= 4.0, "border centre was {border}");
        // The panel line sits above the outer edge, inside the card, by half a border.
        assert!(border < outer_bottom as f32);
    }

    #[test]
    fn panels_stay_clear_of_the_reroll_button() {
        // Same 3440x1440 capture: the reroll button's top border reads y 990, and the frame's outer
        // bottom edge y 959, so the panel's bottom edge belongs in between.
        let client = PixelRect::new(0, 0, 3440, 1440);
        let layout = AugmentLayout::default();

        let bottom = layout.panel_bottom_y(&client);
        assert!(bottom < 990.0, "panel bottom {bottom} would cover the reroll button");
        assert!(bottom > 959.0, "panel bottom {bottom} stops above the frame edge");

        // Growing the panel must move its top, never its bottom.
        let border = layout.bottom_border_y(&client, 0);
        assert!(border < bottom, "the panel grows upward from its bottom edge");
    }

    #[test]
    fn card_centres_match_a_real_capture() {
        // Same capture: the three card centres land on x = 1238, 1728 and 2219.
        let client = PixelRect::new(0, 0, 3440, 1440);
        let layout = AugmentLayout::default();
        for (i, expected) in [1238, 1728, 2219].into_iter().enumerate() {
            let card = layout.card(&client, i);
            let centre = card.x + card.width as i32 / 2;
            assert!((centre - expected).abs() <= 4, "card {i} centre was {centre}, expected {expected}");
        }
    }

    /// A screenshot size and the teal plate of its "hide augments" button, as
    /// `mayhem-vision/tests/augment_button_measure.rs` measured it (left, right, top, bottom).
    type ButtonMeasurement = ((u32, u32), (i32, i32, i32, i32));

    const MEASURED_BUTTONS: [ButtonMeasurement; 6] = [
        ((1024, 768), (444, 579, 593, 625)),
        ((1280, 1024), (549, 731, 789, 834)),
        ((1600, 900), (721, 879, 695, 732)),
        ((1920, 1080), (866, 1056, 833, 880)),
        ((2000, 844), (926, 1075, 650, 687)),
        ((2000, 837), (926, 1075, 645, 683)),
    ];

    /// The predicted button rectangle is the measured plate plus the template's 8 px border, so it
    /// must contain every measured plate and stay concentric with it. Two screen resolutions of the
    /// same height (2000×844 and 2000×837) are in the list on purpose: the button tracks height
    /// only, so those two must predict the same box.
    #[test]
    fn the_button_box_contains_every_measured_plate() {
        let layout = AugmentLayout::default();
        for ((w, h), (left, right, top, bottom)) in MEASURED_BUTTONS {
            let client = PixelRect::new(0, 0, w, h);
            let plate = PixelRect::new(left, top, (right - left + 1) as u32, (bottom - top + 1) as u32);
            let predicted = layout.button(&client);
            assert_eq!(predicted.intersect(&plate), Some(plate), "{w}x{h}: {predicted:?} misses {plate:?}");

            let centre = |r: &PixelRect| (r.x as f32 + r.width as f32 / 2.0, r.y as f32 + r.height as f32 / 2.0);
            let (px, py) = centre(&predicted);
            let (mx, my) = centre(&plate);
            // Three pixels at 1080p: the spread of the measurement itself.
            let tol = 0.003 * h as f32;
            assert!((px - mx).abs() <= tol, "{w}x{h}: centre x off by {}", px - mx);
            assert!((py - my).abs() <= tol, "{w}x{h}: centre y off by {}", py - my);
        }
    }

    /// A screenshot size and the (left, right) frame edges of its three cards.
    type Measurement = ((u32, u32), [(i32, i32); 3]);

    /// Left/right card frame edges measured on the screenshots (bright-frame bounding boxes).
    const MEASURED: [Measurement; 5] = [
        ((1024, 768), [(142, 367), (404, 630), (666, 891)]),
        ((1280, 1024), [(146, 446), (495, 795), (844, 1144)]),
        ((1600, 900), [(366, 630), (673, 937), (980, 1244)]),
        ((1920, 1080), [(440, 756), (807, 1124), (1175, 1492)]),
        ((2000, 844), [(593, 841), (881, 1128), (1168, 1416)]),
    ];

    #[test]
    fn predicted_cards_land_on_measured_cards_at_every_aspect_ratio() {
        let layout = AugmentLayout::default();
        for ((w, h), cards) in MEASURED {
            let client = PixelRect::new(0, 0, w, h);
            assert!(layout.fits(&client));
            for (i, (l, r)) in cards.iter().enumerate() {
                let c = layout.card(&client, i);
                // Within 1% of the height on each edge.
                let tol = (h as f32 * 0.01).ceil() as i32;
                assert!((c.x - l).abs() <= tol, "{w}x{h} card {i}: left {} vs {l}", c.x);
                assert!((c.x + c.width as i32 - r).abs() <= tol, "{w}x{h} card {i}: right vs {r}");
            }
        }
    }

    #[test]
    fn title_band_sits_inside_its_card() {
        let layout = AugmentLayout::default();
        let client = PixelRect::new(100, 50, 1920, 1080);
        for i in 0..OFFER_SLOTS {
            let (card, title) = (layout.card(&client, i), layout.title(&client, i));
            assert_eq!(card.intersect(&title), Some(title));
            assert!(title.y > client.y + 425 && title.y < client.y + 435, "{title:?}");
        }
    }

    /// Every line a wrapped title can take stays inside the card and clear of the icon above it,
    /// and the bands land on the two lines measured at 1920x1200.
    #[test]
    fn a_wrapped_title_block_sits_between_the_icon_and_the_title_band() {
        let layout = AugmentLayout::default();
        let client = PixelRect::new(0, 0, 1920, 1200);
        let lines = layout.title_lines();
        let rise = lines.rise(layout.title(&client, 0).height) as i32;
        assert_eq!(rise, 60, "two more lines at 0.025H");
        for i in 0..OFFER_SLOTS {
            let (card, title, icon) = (layout.card(&client, i), layout.title(&client, i), layout.icon(&client, i));
            let block = PixelRect::new(title.x, title.y - rise, title.width, title.height + rise as u32);
            assert_eq!(card.intersect(&block), Some(block));
            // The icon's art ends at 0.332H on the screenshots; its box is a little larger.
            assert!(block.y > (0.332 * 1200.0) as i32, "{block:?} reaches the icon {icon:?}");
        }
        // Measured text: line one 454-476, line two 484-506. Cutting the block back into bands
        // has to land on both, and the bottom band has to be the title band itself.
        let title = layout.title(&client, 1);
        let block_top = title.y - rise;
        let [two, one] = [0, 1].map(|k| {
            let (top, height) = lines.band(title.height + rise as u32, k);
            (block_top + top as i32, block_top + (top + height) as i32)
        });
        assert_eq!(two, (title.y, title.y + title.height as i32));
        assert!(two.0 <= 484 && two.1 >= 506, "bottom line band {two:?}");
        assert!(one.0 <= 454 && one.1 >= 476, "upper line band {one:?}");
        assert!(one.1 <= 484, "the upper band {one:?} takes in the bottom line's text");

        // One line is one band, wherever the block came from.
        assert_eq!(TitleLines::SINGLE.rise(35), 0);
        assert_eq!(TitleLines::SINGLE.band(35, 0), (0, 35));
    }

    /// The anvil value lines sit inside the card, below the title crop and above each other, at
    /// the heights measured on the seven anvil screenshots.
    #[test]
    fn anvil_value_lines_sit_under_the_title_inside_the_card() {
        let layout = AugmentLayout::default();
        let client = PixelRect::new(0, 0, 1920, 1080);
        for i in 0..OFFER_SLOTS {
            let (card, title) = (layout.card(&client, i), layout.title(&client, i));
            let [one, two] = layout.values(&client, i);
            for line in [one, two] {
                assert_eq!(card.intersect(&line), Some(line));
                assert_eq!(line.width, title.width - 32, "{line:?}");
            }
            assert!(one.y >= title.y + title.height as i32, "{one:?} overlaps {title:?}");
            assert!(two.y >= one.y + one.height as i32, "{two:?} overlaps {one:?}");
            // Measured text at 1080p: line one 506-522, line two 540-556.
            assert!(one.y <= 506 && one.y + one.height as i32 >= 522, "{one:?}");
            assert!(two.y <= 540 && two.y + two.height as i32 >= 556, "{two:?}");
        }
    }

    /// The anvil label ends inside the card with a small gap above the bottom frame, whose inner
    /// edge is at 0.6465H at the lowest on the anvil screenshots.
    #[test]
    fn the_anvil_label_sits_inside_the_card_above_its_bottom_frame() {
        let layout = AugmentLayout::default();
        let client = PixelRect::new(0, 0, 1920, 1080);
        let bottom = layout.anvil_label_bottom_y(&client);
        let frame_inner = 0.6465 * 1080.0;
        let gap = frame_inner - bottom;
        assert!((6.0..=12.0).contains(&gap), "gap above the frame: {gap}px");
        // And above the value lines, with room for the label (~3.5vh) between them.
        let [_, two] = layout.values(&client, 0);
        assert!(bottom - (two.y + two.height as i32) as f32 > 0.05 * 1080.0);
    }

    #[test]
    fn a_window_offset_moves_everything_with_it() {
        let layout = AugmentLayout::default();
        let a = layout.card(&PixelRect::new(0, 0, 1920, 1080), 1);
        let b = layout.card(&PixelRect::new(300, 200, 1920, 1080), 1);
        assert_eq!((b.x - a.x, b.y - a.y), (300, 200));
    }
}
