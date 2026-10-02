//! Finding the in-game "hide augments" button: the signal that the augment screen is open.
//!
//! The card frames ([`crate::frames`]) answer "are the cards drawn right now", which is not the
//! same question as "can augments be picked right now" — the player can hide the cards with this
//! button and the offer is still live. They are also the hardest thing on the screen to find: three
//! colour variants, a Prismatic glow that pulses and washes the outline out (edge correlation
//! 0.47–0.52 against Gold's 0.63–0.75), and a tooltip that covers their lower half.
//!
//! The button has none of those problems. It is one fixed sprite, it does not swap its glyph when
//! toggled, it has no rarity variants and no glow, and it is on screen exactly while augments can
//! be selected. So it, not the frames, decides whether we are in an augment offer at all.
//!
//! Matching is greyscale NCC, which normalises out mean and contrast, so a hover highlight or a
//! different background behind the button does not move the score. It runs at a fixed working width
//! whatever the screen resolution, and over a small region — the button is about 0.19 × 0.06 of the
//! client height — so a tick that finds no button costs well under a millisecond and never touches
//! the card area at all.

use std::sync::Mutex;

use image::imageops::{self, FilterType};
use image::{GrayImage, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::imageops::{locate_ncc, to_gray};
use crate::VisionError;

const BUTTON_PNG: &[u8] = include_bytes!("../assets/button/hide-augments.png");

/// Matching runs at this button width (px); larger captures are downscaled first. The button is
/// 207 px wide at 1080p, so this is a mild downscale there and a large one on a 1440p screen —
/// plenty of detail for a plate with a bright border and a single glyph, at a quarter of the work.
const WORK_WIDTH: u32 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ButtonDetectorConfig {
    /// Minimum correlation for the augment screen to count as open.
    pub present_threshold: f32,
}

impl Default for ButtonDetectorConfig {
    fn default() -> Self {
        // Measured on the seven augment screenshots: the button scores 0.825–0.987 where it is, and
        // the same box shifted onto empty HUD peaks at 0.237 (`button_scores_separate_cleanly`).
        // The bar sits in the middle of that gap. For comparison, the card frames it replaces as
        // the presence signal separate only 0.47 from 0.25.
        Self { present_threshold: 0.60 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonMatch {
    /// Best correlation over the searched offsets.
    pub score: f32,
    pub present: bool,
}

impl ButtonMatch {
    pub fn absent() -> Self {
        Self::default()
    }
}

pub struct ButtonDetector {
    cfg: ButtonDetectorConfig,
    template: RgbaImage,
    /// The template at one working size, greyscale.
    prepared: Mutex<Option<(u32, u32, GrayImage)>>,
}

impl ButtonDetector {
    /// The detector with the button crop bundled in the app.
    pub fn bundled(cfg: ButtonDetectorConfig) -> Result<Self, VisionError> {
        let template = image::load_from_memory_with_format(BUTTON_PNG, image::ImageFormat::Png)?.to_rgba8();
        Ok(Self { cfg, template, prepared: Mutex::new(None) })
    }

    /// `region` is a capture around the predicted button — the layout's rectangle grown by
    /// whatever margin the caller wants searched — and `button_*_px` is the predicted button size
    /// in those pixels. Every placement inside `region` is scored, so the margin needs no separate
    /// parameter: it is the slack already built into `region`.
    pub fn detect(&self, region: &RgbaImage, button_width_px: u32, button_height_px: u32) -> ButtonMatch {
        if button_width_px == 0 || region.width() < button_width_px || region.height() < button_height_px {
            return ButtonMatch::absent();
        }
        // Work at a fixed button width, whatever the screen resolution.
        let scale = (WORK_WIDTH as f32 / button_width_px as f32).min(1.0);
        let work = if scale < 1.0 {
            let w = ((region.width() as f32 * scale).round() as u32).max(1);
            let h = ((region.height() as f32 * scale).round() as u32).max(1);
            to_gray(&imageops::resize(region, w, h, FilterType::Triangle))
        } else {
            to_gray(region)
        };
        let tw = ((button_width_px as f32 * scale).round() as u32).max(8);
        let th = ((button_height_px as f32 * scale).round() as u32).max(8);
        if work.width() < tw || work.height() < th {
            return ButtonMatch::absent();
        }
        let mut guard = self.prepared.lock().unwrap_or_else(|p| p.into_inner());
        if guard.as_ref().is_none_or(|(w, h, _)| (*w, *h) != (tw, th)) {
            let resized = imageops::resize(&self.template, tw, th, FilterType::Triangle);
            *guard = Some((tw, th, to_gray(&resized)));
        }
        let (_, _, template) = guard.as_ref().expect("prepared above");

        let score = locate_ncc(&work, template).map_or(0.0, |p| p.score);
        ButtonMatch { score, present: score >= self.cfg.present_threshold }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::tests::{load, RARITY_SCREENS, SCREENS};
    use crate::imageops::crop_rgba;
    use mayhem_core::geometry::PixelRect;
    use mayhem_core::layout::AugmentLayout;

    /// Search margin around the predicted button, as a fraction of the client height — the same
    /// 1% the card scan allows itself.
    const MARGIN: f32 = 0.01;

    fn detect_at(detector: &ButtonDetector, frame: &RgbaImage, shift_y: i32) -> Option<ButtonMatch> {
        let client = PixelRect::new(0, 0, frame.width(), frame.height());
        let layout = AugmentLayout::default();
        let b = layout.button(&client);
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

    fn all_screens() -> Vec<(&'static str, RgbaImage)> {
        SCREENS.iter().map(|(n, b)| (*n, load(b))).chain(RARITY_SCREENS.iter().map(|(n, b)| (*n, load(b)))).collect()
    }

    /// The measurement behind `present_threshold`: the button where it is, against the same box on
    /// a stretch of screen where it is not.
    #[test]
    fn button_scores_separate_cleanly() {
        let detector = ButtonDetector::bundled(ButtonDetectorConfig::default()).unwrap();
        let (mut lowest_real, mut highest_empty) = (f32::MAX, 0f32);
        for (name, frame) in all_screens() {
            let found = detect_at(&detector, &frame, 0).expect("button box inside the image");
            // Half the button's own height above it: the HUD and the game world, no button.
            let empty = detect_at(&detector, &frame, -(frame.height() as f32 * 0.09) as i32)
                .expect("shifted box inside the image");
            println!("{name}: button {:.3}, empty {:.3}", found.score, empty.score);
            assert!(found.present, "{name}: button not found, {found:?}");
            assert!(!empty.present, "{name}: button found where there is none, {empty:?}");
            lowest_real = lowest_real.min(found.score);
            highest_empty = highest_empty.max(empty.score);
        }
        println!("lowest real {lowest_real:.3}, highest empty {highest_empty:.3}");
        assert!(lowest_real - highest_empty > 0.3, "only {} between signal and noise", lowest_real - highest_empty);
    }

    /// The button is found on a Prismatic offer exactly as well as on a Gold one. This is the whole
    /// point of the change: the glow that costs the card frames a third of their headroom does not
    /// touch the button, because the button does not glow.
    #[test]
    fn the_prismatic_glow_does_not_weaken_the_button() {
        let detector = ButtonDetector::bundled(ButtonDetectorConfig::default()).unwrap();
        let score = |bytes| detect_at(&detector, &load(bytes), 0).unwrap().score;
        let prismatic = score(RARITY_SCREENS[0].1);
        let gold = score(SCREENS[3].1);
        assert!(prismatic > 0.8 && gold > 0.8, "prismatic {prismatic:.3}, gold {gold:.3}");
    }

    #[test]
    fn no_button_on_a_blank_screen() {
        let detector = ButtonDetector::bundled(ButtonDetectorConfig::default()).unwrap();
        let flat = RgbaImage::from_pixel(400, 120, image::Rgba([30, 50, 60, 255]));
        assert!(!detector.detect(&flat, 360, 100).present);
        // A region smaller than the button it is supposed to contain is not a match either.
        assert!(!detector.detect(&flat, 500, 100).present);
    }
}
