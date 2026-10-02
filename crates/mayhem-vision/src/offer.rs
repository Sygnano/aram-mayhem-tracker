//! Reading the three offered augment cards.
//!
//! The pipeline is ordered by cost, and each stage only earns the next one:
//!
//! 1. **The reroll buttons** ([`crate::reroll`]) - a short strip under the cards, a few thousand
//!    pixels. They are present exactly while the cards are drawn, and nothing can cover them.
//! 2. **The "hide augments" button** ([`crate::button`]), only if no reroll was found. It stays on
//!    screen while the cards are hidden, so it answers "an offer is up but put away" - useful to
//!    know, and not the same question as stage 1.
//! 3. **The card area** - the expensive grab - only once one of those said there is something to
//!    look at. Card frames and title crops come from it.
//! 4. **OCR**, only when the title crops actually changed.
//!
//! So a tick during ordinary play costs two small captures and a handful of correlations, and the
//! card area is never read at all.
//!
//! A long title wraps onto a second line, and the recogniser reads one line at a time. So what is
//! cropped per card is a title *block* with room for every line a title can take, and
//! [`title_line_crops`] cuts it into the lines that have text on them before OCR.

use image::RgbaImage;
use mayhem_core::augments::{OfferReading, OFFER_SLOTS};
use mayhem_core::geometry::PixelRect;
use mayhem_core::layout::{AugmentLayout, TitleLines};
use serde::{Deserialize, Serialize};

use crate::button::{ButtonDetector, ButtonDetectorConfig, ButtonMatch};
use crate::capture::ScreenCapture;
use crate::frames::{FrameDetector, FrameDetectorConfig, FrameMatch};
use crate::imageops::{crop_rgba, resize_gray, to_gray};
use crate::matcher::{AugmentMatcher, NameMatch};
use crate::ocr::{OcrEngine, OcrText};
use crate::reroll::{RerollDetector, RerollDetectorConfig, RerollMatch};
use crate::VisionError;

/// Search margin around each predicted card, as a fraction of the client height.
const CARD_MARGIN: f32 = 0.01;
/// Search margin around the predicted "hide augments" button, same units.
const BUTTON_MARGIN: f32 = 0.01;
/// Search margin around each predicted reroll button, same units. Tighter than the others because
/// the reroll box was fitted to the pixel and a wider search only invites false matches.
const REROLL_MARGIN: f32 = 0.008;

/// Luma above which a pixel of a title crop counts as text: the title is near-white on the dark
/// card, and nothing else in the block is.
const TITLE_INK: u8 = 170;

/// The three detectors a scan needs, built once and reused.
pub struct CardDetectors {
    pub rerolls: RerollDetector,
    pub button: ButtonDetector,
    pub frames: FrameDetector,
}

impl CardDetectors {
    /// All three with their bundled templates.
    pub fn bundled(
        frames: FrameDetectorConfig,
        button: ButtonDetectorConfig,
        rerolls: RerollDetectorConfig,
    ) -> Result<Self, VisionError> {
        Ok(Self {
            rerolls: RerollDetector::bundled(rerolls)?,
            button: ButtonDetector::bundled(button)?,
            frames: FrameDetector::bundled(frames)?,
        })
    }
}

/// One look at the augment screen.
#[derive(Debug, Clone)]
pub struct CardScan {
    /// The reroll button under each card. Present exactly while the cards are drawn.
    pub rerolls: [RerollMatch; OFFER_SLOTS],
    /// The "hide augments" button, which stays up even when the cards are put away.
    pub button: ButtonMatch,
    /// Card frames. All absent when the card area was not captured.
    pub frames: [FrameMatch; OFFER_SLOTS],
    /// Title blocks, only when the cards are on screen. Each is the title band with the room above
    /// it that a wrapped title grows into; [`title_line_crops`] cuts it into lines.
    pub titles: Option<[RgbaImage; OFFER_SLOTS]>,
    /// The two value lines under each title, cropped with the titles. Only stat anvil cards have
    /// anything there; they are read only once the titles say the offer is an anvil.
    pub values: Option<[[RgbaImage; 2]; OFFER_SLOTS]>,
}

impl CardScan {
    pub fn absent() -> Self {
        Self {
            rerolls: [RerollMatch::absent(); OFFER_SLOTS],
            button: ButtonMatch::absent(),
            frames: [FrameMatch::absent(); OFFER_SLOTS],
            titles: None,
            values: None,
        }
    }

    /// The cards are drawn, so there are titles to read and somewhere to put the overlay panels.
    ///
    /// **One of anything is enough.** An offer is all-or-nothing - three cards and three reroll
    /// buttons appear and vanish together - so a single reroll button or a single card frame proves
    /// all three are there. Demanding more only loses offers, because whatever hides one card (a
    /// spell effect, the cursor, a champion portrait, the game's own tooltip) does not hide the
    /// rest.
    ///
    /// The two signals are independent on purpose. The rerolls cannot be covered by the tooltip but
    /// do vanish when the cards are hidden; the frames can be covered but are the cards themselves.
    /// Either one alone is enough to be sure.
    pub fn cards_on_screen(&self) -> bool {
        self.rerolls.iter().any(|r| r.present) || self.frames.iter().any(|f| f.present)
    }

    /// An offer is live, whether or not the cards are drawn: the player may have put them away with
    /// the "hide augments" button, which stays on screen when they do.
    pub fn augment_screen_open(&self) -> bool {
        self.cards_on_screen() || self.button.present
    }
}

/// Looks at the augment screen, cheapest question first. See the module docs for the order.
///
/// `force_cards` grabs the card area regardless of what the cheap stages said. The worker uses it
/// for an occasional sweep, so templates that stopped matching after a patch show up as cards found
/// with no buttons, rather than as an app that has silently gone blind.
///
/// The title band is the bottom line of the title, and the crop is grown upwards from it for the
/// lines above.
pub fn scan_cards(
    capture: &mut dyn ScreenCapture,
    client: &PixelRect,
    layout: &AugmentLayout,
    detectors: &CardDetectors,
    force_cards: bool,
) -> Result<CardScan, VisionError> {
    let rerolls = scan_rerolls(capture, client, layout, &detectors.rerolls)?;
    let any_reroll = rerolls.iter().any(|r| r.present);
    // The hide button only answers a question the rerolls left open, so skip it when they didn't.
    let button =
        if any_reroll { ButtonMatch::absent() } else { scan_button(capture, client, layout, &detectors.button)? };

    if !any_reroll && !button.present && !force_cards {
        return Ok(CardScan { rerolls, button, ..CardScan::absent() });
    }
    let mut scan = scan_card_area(capture, client, layout, &detectors.frames)?;
    scan.rerolls = rerolls;
    scan.button = button;
    // The crops were taken before the rerolls were known here; re-apply the rule with them in hand.
    if !scan.cards_on_screen() {
        scan.titles = None;
        scan.values = None;
    }
    Ok(scan)
}

/// Stage one: are the cards drawn? One capture of the reroll strip, then up to three correlations.
///
/// The search stops at the first button found, since one reroll means all three cards are up.
fn scan_rerolls(
    capture: &mut dyn ScreenCapture,
    client: &PixelRect,
    layout: &AugmentLayout,
    detector: &RerollDetector,
) -> Result<[RerollMatch; OFFER_SLOTS], VisionError> {
    let margin = (client.height as f32 * REROLL_MARGIN).round() as i32;
    let boxes = layout.rerolls(client);
    let grown = boxes
        .map(|b| PixelRect::new(b.x - margin, b.y - margin, b.width + 2 * margin as u32, b.height + 2 * margin as u32));
    let bounds = grown.iter().copied().reduce(|a, b| a.union(&b)).expect("non-empty");
    let Some(bounds) = bounds.intersect(client).filter(|r| *r == bounds) else {
        return Ok([RerollMatch::absent(); OFFER_SLOTS]);
    };
    let grab = capture.capture(&bounds)?;

    let mut out = [RerollMatch::absent(); OFFER_SLOTS];
    for i in 0..OFFER_SLOTS {
        let Some(region) = crop_rgba(&grab, &grown[i].relative_to(&bounds)) else { continue };
        out[i] = detector.detect(&region, boxes[i].width, boxes[i].height);
        if out[i].present {
            break;
        }
    }
    Ok(out)
}

/// Stage two: is an offer up with the cards put away?
fn scan_button(
    capture: &mut dyn ScreenCapture,
    client: &PixelRect,
    layout: &AugmentLayout,
    detector: &ButtonDetector,
) -> Result<ButtonMatch, VisionError> {
    let margin = (client.height as f32 * BUTTON_MARGIN).round() as i32;
    let b = layout.button(client);
    let grown = PixelRect::new(b.x - margin, b.y - margin, b.width + 2 * margin as u32, b.height + 2 * margin as u32);
    // A window small enough to clip the button box is a window we cannot judge from.
    let Some(bounds) = grown.intersect(client).filter(|r| *r == grown) else {
        return Ok(ButtonMatch::absent());
    };
    let grab = capture.capture(&bounds)?;
    Ok(detector.detect(&grab, b.width, b.height))
}

/// Stage three: grabs the card area once, checks the card frames, and crops the title bands from
/// the same grab.
fn scan_card_area(
    capture: &mut dyn ScreenCapture,
    client: &PixelRect,
    layout: &AugmentLayout,
    detector: &FrameDetector,
) -> Result<CardScan, VisionError> {
    let margin = (client.height as f32 * CARD_MARGIN).round() as i32;
    let grown = layout
        .cards(client)
        .map(|c| PixelRect::new(c.x - margin, c.y - margin, c.width + 2 * margin as u32, c.height + 2 * margin as u32));
    // A wrapped title keeps its last line in the band and stacks the others above it.
    let lines = layout.title_lines();
    let titles = layout.titles(client).map(|t| {
        let rise = lines.rise(t.height);
        PixelRect::new(t.x, t.y - rise as i32, t.width, t.height + rise)
    });
    let bounds = grown.iter().chain(&titles).copied().reduce(|a, b| a.union(&b)).expect("non-empty");
    let bounds = bounds.intersect(client).ok_or(VisionError::RegionOutOfBounds)?;
    let grab = capture.capture(&bounds)?;

    let cards = layout.cards(client);
    let frames: [FrameMatch; OFFER_SLOTS] = std::array::from_fn(|i| {
        crop_rgba(&grab, &grown[i].relative_to(&bounds))
            .map(|region| detector.detect(&region, cards[i].width, cards[i].height, margin as u32))
            .unwrap_or_else(FrameMatch::absent)
    });
    let mut scan = CardScan { frames, ..CardScan::absent() };
    // Crop the titles whenever there is any sign of a card; `scan_cards` decides for real.
    if scan.frames.iter().any(|f| f.present) {
        let crops: Option<Vec<RgbaImage>> = titles.iter().map(|t| crop_rgba(&grab, &t.relative_to(&bounds))).collect();
        scan.titles = crops.and_then(|c| c.try_into().ok());
        // The value lines are inside the cards, so inside the grab. Cropping them is a copy of a
        // few thousand pixels; reading them waits until the titles say this is an anvil.
        let values: Option<Vec<[RgbaImage; 2]>> = (0..OFFER_SLOTS)
            .map(|i| {
                let [a, b] = layout.values(client, i);
                Some([crop_rgba(&grab, &a.relative_to(&bounds))?, crop_rgba(&grab, &b.relative_to(&bounds))?])
            })
            .collect();
        scan.values = values.and_then(|v| v.try_into().ok());
    }
    Ok(scan)
}

/// The result of one pass, with the raw text kept for the settings screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferRead {
    pub reading: OfferReading,
    pub texts: [String; OFFER_SLOTS],
    pub matches: [Option<NameMatch>; OFFER_SLOTS],
}

/// Cuts a title block into the lines that have text on them, top line first.
///
/// The lines are *placed*, not searched for. A wrapped title keeps its last line where a one-line
/// title is and stacks the others above it at a fixed pitch, so each line's band is known in
/// advance; finding lines by the gaps between them would have two or three pixels to go on at
/// 768p, where a descender above nearly meets an accent below.
///
/// The bottom band is always returned, so a block with nothing recognisable in it is read exactly
/// as the single title band used to be. A band above it is returned only if it has text, and only
/// if the band below it did: a title has no blank line in the middle.
pub fn title_line_crops(block: &RgbaImage, lines: TitleLines) -> Vec<RgbaImage> {
    let mut out = Vec::new();
    for k in 0..lines.max.max(1) {
        let (top, height) = lines.band(block.height(), k);
        let Some(crop) = crop_rgba(block, &PixelRect::new(0, top as i32, block.width(), height)) else { break };
        if k > 0 && !has_text(&crop) {
            break;
        }
        out.push(crop);
    }
    out.reverse();
    out
}

/// Whether a line band holds a line of text.
///
/// Only the middle of the band is looked at, where the body of every letter is. The rows at its
/// edges belong as much to the neighbouring lines: the descenders of the line above and the accents
/// of the line below reach into them, and must not make an empty band look written on.
fn has_text(band: &RgbaImage) -> bool {
    let gray = to_gray(band);
    let rows = (gray.height() as f32 * 0.2).round() as u32..(gray.height() as f32 * 0.8).round() as u32;
    let ink = rows
        .flat_map(|y| (0..gray.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| gray.get_pixel(x, y).0[0] > TITLE_INK)
        .count();
    // A single short word is several times this; a stray highlight is not.
    ink >= band.height() as usize
}

/// OCR the three title blocks and match them against the augment vocabulary.
///
/// Each block is cut into its lines first (`lines` says how they are stacked), every line of every
/// card is recognised in one batch, and a card's lines are joined back into its title. A line is
/// never matched on its own: the last line of one name can be the whole of another.
///
/// No rarity is passed in: the matched *names* carry their own tier, so nothing has to be inferred
/// from a frame colour that the Prismatic glow makes unreliable.
pub fn read_offer(
    engine: &dyn OcrEngine,
    matcher: &AugmentMatcher,
    blocks: &[RgbaImage; OFFER_SLOTS],
    lines: TitleLines,
) -> Result<OfferRead, VisionError> {
    let per_card = blocks.each_ref().map(|b| title_line_crops(b, lines));
    let counts = per_card.each_ref().map(Vec::len);
    let crops: Vec<RgbaImage> = per_card.into_iter().flatten().collect();
    let mut recognised = engine.recognize_all(&crops)?.into_iter();
    let texts: [String; OFFER_SLOTS] = counts.map(|n| {
        let read: Vec<String> =
            recognised.by_ref().take(n).flat_map(|t: OcrText| t.lines).filter(|l| !l.trim().is_empty()).collect();
        read.join(" ")
    });
    let matches = matcher.match_offer(&texts);
    let reading = OfferReading { slots: matches.map(|m| m.map(|m| m.id)) };
    Ok(OfferRead { reading, texts, matches })
}

/// A tiny greyscale thumbnail of each title block. When none has changed since the last OCR pass,
/// the previous result still stands and OCR is skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint(Vec<u8>);

impl Fingerprint {
    const W: u32 = 32;
    const H: u32 = 8;
    /// Horizontal stripes per block, each compared on its own.
    const STRIPES: u32 = 3;
    const PER_STRIPE: usize = (Self::W * Self::H) as usize;

    pub fn of(blocks: &[RgbaImage; OFFER_SLOTS]) -> Self {
        let mut v = Vec::with_capacity(Self::PER_STRIPE * (Self::STRIPES as usize) * OFFER_SLOTS);
        for block in blocks {
            v.extend_from_slice(resize_gray(&to_gray(block), Self::W, Self::H * Self::STRIPES).as_raw());
        }
        Self(v)
    }

    /// The largest mean absolute difference of any one stripe of any one card, 0..255.
    ///
    /// Per card, not over the whole offer: a reroll replaces one title of three, so averaged across
    /// all three its difference is diluted threefold and could fall under the change threshold. The
    /// reroll then went unnoticed until the periodic re-read came round, which is what made a
    /// rerolled augment take seconds to refresh.
    ///
    /// Per stripe for the same reason: a block has room for three lines and most titles fill one,
    /// so a changed one-line title averaged over its whole block is diluted threefold again.
    pub fn distance(&self, other: &Fingerprint) -> f32 {
        if self.0.len() != other.0.len() || self.0.is_empty() {
            return 255.0;
        }
        self.0
            .chunks(Self::PER_STRIPE)
            .zip(other.0.chunks(Self::PER_STRIPE))
            .map(|(a, b)| {
                let sum: u64 = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b) as u64).sum();
                sum as f32 / a.len() as f32
            })
            .fold(0.0, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::StillCapture;
    use crate::frames::tests::{load, ALL_OFFER_SCREENS, SCREENS};
    use crate::matcher::{AugmentName, MatcherConfig};
    use crate::ocr::ScriptedOcr;
    use image::Rgba;

    fn detectors() -> CardDetectors {
        CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap()
    }

    /// Counts what the pipeline actually reads off the screen, so the staged scan's cost can be
    /// asserted rather than assumed.
    struct CountingCapture {
        inner: StillCapture,
        grabs: Vec<PixelRect>,
    }

    impl ScreenCapture for CountingCapture {
        fn game_client_rect(&mut self) -> Result<PixelRect, VisionError> {
            self.inner.game_client_rect()
        }
        fn capture(&mut self, rect: &PixelRect) -> Result<RgbaImage, VisionError> {
            self.grabs.push(*rect);
            self.inner.capture(rect)
        }
    }

    fn counting(frame: RgbaImage) -> CountingCapture {
        CountingCapture { inner: StillCapture { frame }, grabs: Vec::new() }
    }

    fn blank() -> RgbaImage {
        RgbaImage::from_pixel(1920, 1080, Rgba([30, 50, 60, 255]))
    }

    #[test]
    fn reads_and_matches_three_cards() {
        let matcher = AugmentMatcher::new(
            MatcherConfig::default(),
            &[
                AugmentName { id: 1, name: "Back To Basics".into(), rarity: "kPrismatic".into() },
                AugmentName { id: 2, name: "Draw Your Sword".into(), rarity: "kPrismatic".into() },
                AugmentName { id: 3, name: "DropBear".into(), rarity: "kPrismatic".into() },
            ],
        );
        let ocr = ScriptedOcr::new(["BACK TO\nBASICS", "", "Drop Bear"]);
        let crops = [RgbaImage::new(40, 10), RgbaImage::new(40, 10), RgbaImage::new(40, 10)];
        let read = read_offer(&ocr, &matcher, &crops, AugmentLayout::default().title_lines()).unwrap();
        assert_eq!(read.reading.slots, [Some(1), None, Some(3)]);
        assert_eq!(read.texts[0], "BACK TO BASICS");
    }

    /// A title block of `lines` bands with white bars drawn across the given lines (0 = bottom).
    fn block_with_text(lines: TitleLines, written: &[u32]) -> RgbaImage {
        let band = 30;
        let height = band + lines.rise(band);
        let mut block = RgbaImage::from_pixel(240, height, Rgba([10, 30, 34, 255]));
        for &k in written {
            let (top, h) = lines.band(height, k);
            for y in top + h / 3..top + 2 * h / 3 {
                for x in 60..180 {
                    block.put_pixel(x, y, Rgba([240, 240, 240, 255]));
                }
            }
        }
        block
    }

    #[test]
    fn a_block_is_cut_into_the_lines_that_have_text() {
        let lines = AugmentLayout::default().title_lines();
        let count = |written: &[u32]| title_line_crops(&block_with_text(lines, written), lines).len();
        assert_eq!(count(&[0]), 1);
        assert_eq!(count(&[0, 1]), 2);
        assert_eq!(count(&[0, 1, 2]), 3);
        // The bottom band is read whatever is in it, as the single title band always was.
        assert_eq!(count(&[]), 1);
        // No blank line in the middle of a title: something two lines up is not part of it.
        assert_eq!(count(&[0, 2]), 1);
        // Each crop is one band.
        let crops = title_line_crops(&block_with_text(lines, &[0, 1]), lines);
        assert!(crops.iter().all(|c| c.dimensions() == (240, 30)));
        // A block that is a single band is never cut.
        assert_eq!(title_line_crops(&RgbaImage::new(240, 30), TitleLines::SINGLE).len(), 1);
    }

    /// One OCR call per line, top to bottom, card by card; a card's lines become one title.
    #[test]
    fn a_wrapped_title_is_read_line_by_line_and_joined() {
        let lines = AugmentLayout::default().title_lines();
        let n = |id, name: &str| AugmentName { id, name: name.into(), rarity: "kSilver".into() };
        let matcher = AugmentMatcher::new(
            MatcherConfig::default(),
            &[n(1, "Gros cerveau"), n(2, "Expertise en omnivampirisme"), n(3, "Machine à rétrécir")],
        );
        let blocks = [block_with_text(lines, &[0]), block_with_text(lines, &[0, 1]), block_with_text(lines, &[0])];
        let ocr = ScriptedOcr::new(["Gros cerveau", "Expertise en", "omnivampirisme", "Machine a retrecir"]);
        let read = read_offer(&ocr, &matcher, &blocks, lines).unwrap();
        assert_eq!(read.texts[1], "Expertise en omnivampirisme");
        assert_eq!(read.reading.slots, [Some(1), Some(2), Some(3)]);
    }

    /// Something bright above a one-line title (the cursor, say) is read as a line of rubbish, or
    /// as nothing. Either way the title under it is still named.
    #[test]
    fn rubbish_above_a_one_line_title_does_not_lose_it() {
        let lines = AugmentLayout::default().title_lines();
        let n = |id, name: &str| AugmentName { id, name: name.into(), rarity: "kSilver".into() };
        let matcher = AugmentMatcher::new(
            MatcherConfig::default(),
            &[n(1, "Gros cerveau"), n(2, "Vampirisme"), n(3, "Machine à rétrécir")],
        );
        let blocks = [block_with_text(lines, &[0, 1]), block_with_text(lines, &[0, 1]), block_with_text(lines, &[0])];
        let ocr = ScriptedOcr::new(["k/7", "Gros cerveau", "", "Vampirisme", "Machine a retrecir"]);
        let read = read_offer(&ocr, &matcher, &blocks, lines).unwrap();
        assert_eq!(read.reading.slots, [Some(1), Some(2), Some(3)]);
        assert_eq!(read.texts[1], "Vampirisme");
    }

    /// The whole pipeline on every real offer we have: rerolls found, cards called on screen, and
    /// title crops taken that hold their text without clipping it.
    #[test]
    fn scans_every_real_screenshot_into_title_crops() {
        let detectors = detectors();
        let layout = AugmentLayout::default();
        for (name, bytes) in ALL_OFFER_SCREENS {
            let mut cap = counting(load(bytes));
            let client = cap.game_client_rect().unwrap();
            let scan = scan_cards(&mut cap, &client, &layout, &detectors, false).unwrap();
            assert!(scan.rerolls.iter().any(|r| r.present), "{name}: no reroll found");
            assert!(scan.cards_on_screen(), "{name}");
            assert!(scan.augment_screen_open(), "{name}");
            // Two grabs: the reroll strip, then the card area. The hide button is never consulted,
            // because the rerolls already answered the question it exists to answer.
            assert_eq!(cap.grabs.len(), 2, "{name}: reroll strip then card area");

            let titles = scan.titles.expect("title crops when cards are on screen");
            for (i, t) in titles.iter().enumerate() {
                // Every title in these screenshots is one line, so one line is what the block must
                // be cut into: nothing else on the card may pass for a line above it.
                let lines = title_line_crops(t, layout.title_lines());
                assert_eq!(lines.len(), 1, "{name} card {i}: a one-line title cut into {} lines", lines.len());
                // The title is light text on the dark card: the crop must contain some, and the
                // text must not be cut by the crop's top or bottom edge.
                let gray = to_gray(&lines[0]);
                let bright_rows: Vec<u32> = (0..gray.height())
                    .filter(|&y| (0..gray.width()).any(|x| gray.get_pixel(x, y).0[0] > 170))
                    .collect();
                assert!(bright_rows.len() >= 3, "{name} card {i}: no title text in the crop");
                assert!(
                    *bright_rows.first().unwrap() > 0 && *bright_rows.last().unwrap() < gray.height() - 1,
                    "{name} card {i}: title touches the crop edge"
                );
            }
        }
    }

    #[test]
    fn a_frame_without_cards_yields_no_titles() {
        let detectors = detectors();
        let mut cap = StillCapture { frame: blank() };
        let client = cap.game_client_rect().unwrap();
        let scan = scan_cards(&mut cap, &client, &AugmentLayout::default(), &detectors, false).unwrap();
        assert!(!scan.cards_on_screen());
        assert!(!scan.augment_screen_open());
        assert!(scan.titles.is_none());
    }

    /// Outside an augment offer - which is nearly all of a game - the scan must read the reroll
    /// strip and the hide-augments button and stop. Capturing and correlating the card area every
    /// tick was the old cost, and the staged scan exists to remove it.
    #[test]
    fn without_any_button_the_card_area_is_never_captured() {
        let detectors = detectors();
        let layout = AugmentLayout::default();
        let mut cap = counting(blank());
        let client = cap.game_client_rect().unwrap();
        let scan = scan_cards(&mut cap, &client, &layout, &detectors, false).unwrap();

        assert!(!scan.augment_screen_open());
        assert_eq!(cap.grabs.len(), 2, "grabbed more than the two small boxes: {:?}", cap.grabs);
        let area = |r: &PixelRect| r.width as u64 * r.height as u64;
        let cheap: u64 = cap.grabs.iter().map(area).sum();
        let cards = layout.cards(&client).iter().copied().reduce(|a, b| a.union(&b)).unwrap();
        assert!(cheap * 4 < area(&cards), "cheap path {cheap} px against a card area of {} px", area(&cards));

        // The sweep is the deliberate exception: it looks at the cards without any button.
        let mut cap = counting(blank());
        scan_cards(&mut cap, &client, &layout, &detectors, true).unwrap();
        assert_eq!(cap.grabs.len(), 3, "the sweep should grab the card area too");
    }

    /// The tooltip case that sank the hide-augments button as the gate: the player hovers a card,
    /// the game draws a tooltip over the button, and the cards are plainly still there. The rerolls
    /// sit below the tooltip and carry the scan on their own.
    #[test]
    fn a_tooltip_over_the_hide_button_does_not_hide_the_offer() {
        let detectors = detectors();
        let layout = AugmentLayout::default();
        for (name, bytes) in SCREENS {
            let mut frame = load(bytes);
            let client = PixelRect::new(0, 0, frame.width(), frame.height());
            // Paint over the hide-augments button, and over the lower half of the cards, which is
            // how far a long tooltip reaches.
            let b = layout.button(&client);
            let cards = layout.cards(&client);
            let covered = [
                b,
                PixelRect::new(
                    cards[0].x,
                    cards[0].y + (cards[0].height as f32 * 0.45) as i32,
                    (cards[2].x + cards[2].width as i32 - cards[0].x) as u32,
                    cards[0].height / 2,
                ),
            ];
            for r in covered {
                for y in r.y.max(0) as u32..((r.y + r.height as i32) as u32).min(frame.height()) {
                    for x in r.x.max(0) as u32..((r.x + r.width as i32) as u32).min(frame.width()) {
                        frame.put_pixel(x, y, Rgba([20, 22, 28, 255]));
                    }
                }
            }
            let mut cap = StillCapture { frame };
            let scan = scan_cards(&mut cap, &client, &layout, &detectors, false).unwrap();
            assert!(!scan.button.present, "{name}: the painted-over button should not match");
            assert!(scan.rerolls.iter().any(|r| r.present), "{name}: rerolls carry the scan");
            assert!(scan.cards_on_screen(), "{name}");
            assert!(scan.titles.is_some(), "{name}");
        }
    }

    /// One of anything is enough, and neither signal needs the other.
    #[test]
    fn one_reroll_or_one_frame_is_enough() {
        use crate::frames::FrameMatch;
        let card = FrameMatch { present: true, score: 0.7, ..FrameMatch::absent() };
        let reroll = RerollMatch { score: 0.9, present: true, spent: false };
        let scan = |rerolls: usize, frames: usize, button: bool| CardScan {
            rerolls: std::array::from_fn(|i| if i < rerolls { reroll } else { RerollMatch::absent() }),
            frames: std::array::from_fn(|i| if i < frames { card } else { FrameMatch::absent() }),
            button: ButtonMatch { score: if button { 0.9 } else { 0.1 }, present: button },
            titles: None,
            values: None,
        };
        assert!(scan(1, 0, false).cards_on_screen(), "one reroll, no frames");
        assert!(scan(0, 1, false).cards_on_screen(), "one frame, no rerolls");
        assert!(!scan(0, 0, false).cards_on_screen());

        // The hide button alone means an offer is up with the cards put away: worth knowing, but
        // there is nothing on screen to draw a panel on.
        assert!(!scan(0, 0, true).cards_on_screen());
        assert!(scan(0, 0, true).augment_screen_open());
        assert!(scan(1, 0, false).augment_screen_open(), "cards up implies an offer is up");
    }

    #[test]
    fn fingerprint_detects_change() {
        let a = [RgbaImage::new(40, 10), RgbaImage::new(40, 10), RgbaImage::new(40, 10)];
        let mut b = a.clone();
        for p in b[1].pixels_mut() {
            *p = Rgba([255, 255, 255, 255]);
        }
        assert_eq!(Fingerprint::of(&a).distance(&Fingerprint::of(&a)), 0.0);
        // One card of three changing must register at full strength, not a third of it.
        assert_eq!(Fingerprint::of(&a).distance(&Fingerprint::of(&b)), 255.0);
    }

    /// A reroll: one real title crop swapped for a different one, the other two untouched.
    #[test]
    fn one_rerolled_card_is_seen_as_a_change() {
        use crate::capture::StillCapture;
        use crate::frames::tests::load;
        let detectors = CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap();
        let mut cap = StillCapture { frame: load(crate::frames::tests::SCREENS[3].1) };
        let client = cap.game_client_rect().unwrap();
        let titles = scan_cards(&mut cap, &client, &AugmentLayout::default(), &detectors, false)
            .unwrap()
            .titles
            .expect("cards on screen");
        // "Vulnerability" in slot 1 replaced by the "Combusting Interest" crop.
        let mut rerolled = titles.clone();
        rerolled[1] = titles[2].clone();
        let moved = Fingerprint::of(&titles).distance(&Fingerprint::of(&rerolled));
        assert!(moved > 4.0, "a rerolled card moved the fingerprint by only {moved}");
    }
}
