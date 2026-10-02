//! Finding the reroll buttons under the augment cards.
//!
//! These are the sturdiest thing on the augment screen, and the reason they beat both of the
//! alternatives is what they *cannot* have happen to them:
//!
//! | | covered by the augment tooltip | rarity variants | glow / pulse | hidden with the cards |
//! |---|---|---|---|---|
//! | Card frame | **yes**, its lower half | three | **yes** | yes |
//! | "Hide augments" button | **yes** | no | no | **no** |
//! | **Reroll button** | **no** | no | no | yes |
//!
//! The tooltip is what sank the hide-augments button as a gate: hovering a card — which is exactly
//! when the player wants to see our numbers — draws a tooltip over it. The reroll buttons sit in a
//! row below the cards that the tooltip does not reach.
//!
//! They have two appearances, unused and spent, and both are bundled as templates cut from a real
//! 3440x1440 screenshot. Matching is greyscale NCC, which normalises out mean and contrast, so the
//! difference between the two is smaller than it looks — but not small enough to risk one template,
//! so both are tried and the better score wins.
//!
//! Because the three rerolls appear and vanish together with the cards, **finding one is finding
//! all three**, and the search stops at the first hit.

use std::sync::Mutex;

use image::imageops::{self, FilterType};
use image::{GrayImage, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::imageops::{locate_ncc, to_gray};
use crate::VisionError;

const UNUSED_PNG: &[u8] = include_bytes!("../assets/reroll/unused.png");
const SPENT_PNG: &[u8] = include_bytes!("../assets/reroll/spent.png");

/// Matching runs at this button width (px); larger captures are downscaled first. The plate is
/// 103 px wide at 1440p, so this is a real downscale there and roughly native at 720p.
const WORK_WIDTH: u32 = 56;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RerollDetectorConfig {
    /// Minimum correlation for a reroll button to count as present.
    pub present_threshold: f32,
}

impl Default for RerollDetectorConfig {
    fn default() -> Self {
        // Swept over every augment screenshot we have (`tests/reroll_measure.rs`): the worst real
        // button scores 0.773 — a *spent* one, the hardest case — while the best false match found
        // by sliding the box over the whole screen reaches 0.452. The bar sits between them.
        // Production only needs the best of the three, and that worst case is 0.867.
        Self { present_threshold: 0.60 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerollMatch {
    /// Best correlation over both templates and all offsets.
    pub score: f32,
    pub present: bool,
    /// Which template won: the reroll has been used. Reported for the settings screen only — nothing
    /// depends on it, since a spent reroll is just as good a sign that the cards are up.
    pub spent: bool,
}

impl RerollMatch {
    pub fn absent() -> Self {
        Self::default()
    }
}

pub struct RerollDetector {
    cfg: RerollDetectorConfig,
    /// Unused, then spent.
    templates: [RgbaImage; 2],
    /// Both templates at one working size, greyscale.
    prepared: Mutex<Option<(u32, u32, [GrayImage; 2])>>,
}

impl RerollDetector {
    /// The detector with the two button crops bundled in the app.
    pub fn bundled(cfg: RerollDetectorConfig) -> Result<Self, VisionError> {
        let load = |bytes| -> Result<RgbaImage, VisionError> {
            Ok(image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.to_rgba8())
        };
        Ok(Self::new(cfg, [load(UNUSED_PNG)?, load(SPENT_PNG)?]))
    }

    /// The detector with templates supplied rather than bundled, which is how the bundled ones were
    /// chosen: `tests/reroll_measure.rs` sweeps candidate crops through this and keeps the best.
    pub fn new(cfg: RerollDetectorConfig, templates: [RgbaImage; 2]) -> Self {
        Self { cfg, templates, prepared: Mutex::new(None) }
    }

    /// `region` is a capture around one predicted reroll button, grown by whatever margin the
    /// caller wants searched; `button_*_px` is the predicted plate size in those pixels.
    pub fn detect(&self, region: &RgbaImage, button_width_px: u32, button_height_px: u32) -> RerollMatch {
        if button_width_px == 0 || region.width() < button_width_px || region.height() < button_height_px {
            return RerollMatch::absent();
        }
        let scale = (WORK_WIDTH as f32 / button_width_px as f32).min(1.0);
        let work = if scale < 1.0 {
            let w = ((region.width() as f32 * scale).round() as u32).max(1);
            let h = ((region.height() as f32 * scale).round() as u32).max(1);
            to_gray(&imageops::resize(region, w, h, FilterType::Triangle))
        } else {
            to_gray(region)
        };
        let tw = ((button_width_px as f32 * scale).round() as u32).max(6);
        let th = ((button_height_px as f32 * scale).round() as u32).max(6);
        if work.width() < tw || work.height() < th {
            return RerollMatch::absent();
        }

        let mut guard = self.prepared.lock().unwrap_or_else(|p| p.into_inner());
        if guard.as_ref().is_none_or(|(w, h, _)| (*w, *h) != (tw, th)) {
            let resized = self.templates.clone().map(|t| to_gray(&imageops::resize(&t, tw, th, FilterType::Triangle)));
            *guard = Some((tw, th, resized));
        }
        let (_, _, templates) = guard.as_ref().expect("prepared above");

        let unused = locate_ncc(&work, &templates[0]).map_or(0.0, |p| p.score);
        let spent = locate_ncc(&work, &templates[1]).map_or(0.0, |p| p.score);
        let score = unused.max(spent);
        RerollMatch { score, present: score >= self.cfg.present_threshold, spent: spent > unused }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::tests::{load, ALL_OFFER_SCREENS};
    use crate::imageops::crop_rgba;
    use mayhem_core::geometry::PixelRect;
    use mayhem_core::layout::AugmentLayout;

    /// Search margin around each predicted reroll, as a fraction of the client height.
    const MARGIN: f32 = 0.008;

    fn detect_at(detector: &RerollDetector, frame: &RgbaImage, i: usize, shift_y: i32) -> Option<RerollMatch> {
        let client = PixelRect::new(0, 0, frame.width(), frame.height());
        let b = AugmentLayout::default().reroll(&client, i);
        let margin = (client.height as f32 * MARGIN).round() as i32;
        let grown = PixelRect::new(
            b.x - margin,
            b.y - margin + shift_y,
            b.width + 2 * margin as u32,
            b.height + 2 * margin as u32,
        );
        let region = crop_rgba(frame, &grown)?;
        Some(detector.detect(&region, b.width, b.height))
    }

    /// The measurement behind `present_threshold`. Every real offer we have — twelve screenshots,
    /// five resolutions with a spent reroll among them — against the same boxes moved onto a
    /// stretch of screen with no buttons in it.
    #[test]
    fn reroll_scores_separate_cleanly() {
        let detector = RerollDetector::bundled(RerollDetectorConfig::default()).unwrap();
        let (mut lowest_real, mut highest_empty) = (f32::MAX, 0f32);
        for (name, bytes) in ALL_OFFER_SCREENS {
            let frame = load(bytes);
            let scores: Vec<RerollMatch> = (0..3).filter_map(|i| detect_at(&detector, &frame, i, 0)).collect();
            let shown: Vec<String> =
                scores.iter().map(|m| format!("{:.3}{}", m.score, if m.spent { "s" } else { "" })).collect();
            println!("{name}: {}", shown.join(" "));
            for (i, m) in scores.iter().enumerate() {
                assert!(m.present, "{name} reroll {i}: {m:?}");
                lowest_real = lowest_real.min(m.score);
            }
            // A third of a card height above the row: cards and scenery, no reroll buttons.
            let up = -(frame.height() as f32 * 0.16) as i32;
            for (i, m) in (0..3).filter_map(|i| detect_at(&detector, &frame, i, up)).enumerate() {
                assert!(!m.present, "{name} reroll {i} found above the row: {m:?}");
                highest_empty = highest_empty.max(m.score);
            }
        }
        println!("lowest real {lowest_real:.3}, highest empty {highest_empty:.3}");
        assert!(lowest_real - highest_empty > 0.15, "only {} between signal and noise", lowest_real - highest_empty);
    }

    /// The spent reroll in the five 2026-09-30 screenshots is card 0. It must be found just as
    /// surely as the unused ones, and be reported as spent.
    #[test]
    fn a_spent_reroll_is_found_and_labelled() {
        use crate::frames::tests::REROLL_SCREENS;
        let detector = RerollDetector::bundled(RerollDetectorConfig::default()).unwrap();
        for (name, bytes) in REROLL_SCREENS {
            let frame = load(bytes);
            let spent = detect_at(&detector, &frame, 0, 0).unwrap();
            assert!(spent.present && spent.spent, "{name} card 0 should be a spent reroll: {spent:?}");
            for i in [1, 2] {
                let m = detect_at(&detector, &frame, i, 0).unwrap();
                assert!(m.present && !m.spent, "{name} card {i} should be an unused reroll: {m:?}");
            }
        }
    }

    #[test]
    fn no_reroll_on_a_blank_screen() {
        let detector = RerollDetector::bundled(RerollDetectorConfig::default()).unwrap();
        let flat = RgbaImage::from_pixel(200, 120, image::Rgba([30, 50, 60, 255]));
        assert!(!detector.detect(&flat, 160, 90).present);
        assert!(!detector.detect(&flat, 400, 90).present);
    }
}
