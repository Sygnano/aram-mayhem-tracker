//! Card frame detection: are the augment cards drawn on screen?
//!
//! Every augment card is drawn inside one of three frames (Silver, Gold, Prismatic), baked into the
//! app as 310×512 PNGs with a transparent centre (supplied by the project owner). The three share
//! one outline, so only one question is asked of them: **is a card here?**
//!
//! Presence is normalised cross-correlation of **edge strength** (gradient magnitude) between the
//! screen and each frame, over a band around the frame only (the card's contents are masked out).
//! Edges rather than brightness, because the game does not draw the frames like the PNGs: the
//! Prismatic frame is rendered almost white with a strong glow, which defeats brightness matching
//! but keeps the outline. Measured at the working size: Gold cards 0.63–0.75 at five resolutions,
//! Prismatic cards 0.47–0.52 on a downscaled desktop screenshot, the same spots without a card
//! ≤ 0.19.
//!
//! **Rarity is no longer read here**. It used to come from the frame's broad colour, and
//! that was the cause of a real bug: a Prismatic frame at the peak of its glow blows out to white,
//! white carries no hue to classify, and the card was read as Silver — which then restricted name
//! matching to the wrong tier. Rarity now comes from the augment's *name*, which the vocabulary
//! already knows, so nothing has to be inferred from a pulsing glow.
//!
//! The frames are also no longer the primary sign that an offer is up; the reroll buttons are
//! ([`crate::reroll`]), because they cannot be covered by the game's augment tooltip. The frames
//! remain as a second, independent way to see that the cards are drawn.

use std::sync::Mutex;

use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::VisionError;

const SILVER_PNG: &[u8] = include_bytes!("../assets/frames/silver.png");
const GOLD_PNG: &[u8] = include_bytes!("../assets/frames/gold.png");
const PRISMATIC_PNG: &[u8] = include_bytes!("../assets/frames/prismatic.png");

/// Matching runs at this card height (px); larger captures are downscaled first.
const WORK_HEIGHT: u32 = 192;
/// Template pixels with at least this alpha belong to the frame.
const OPAQUE: u8 = 128;
/// The card's dark interior, composited behind the templates so their inner edges exist.
const CARD_INTERIOR: Rgba<u8> = Rgba([13, 26, 28, 255]);
/// Half-width (px, at working size) of the band around the frame that edges are compared over.
const BAND: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FrameDetectorConfig {
    /// Minimum edge correlation for a card to count as present.
    pub present_threshold: f32,
    /// How much of the card, from the top down, the outline is matched over.
    ///
    /// The game's own augment tooltip covers the lower part of the cards whenever it is long, which
    /// used to drop the match below threshold and close the offer while it was still on screen.
    /// The top of the frame is plenty to tell a card from no card.
    pub top_fraction: f32,
}

impl Default for FrameDetectorConfig {
    fn default() -> Self {
        // Presence: halfway between the highest no-card score (0.19) and the lowest real card (0.47).
        Self { present_threshold: 0.35, top_fraction: 0.55 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameMatch {
    /// Best edge correlation over all templates and offsets.
    pub score: f32,
    /// Best correlation per template: Silver, Gold, Prismatic. Kept for debugging only — the three
    /// outlines are near-identical, so these scores do not identify the rarity.
    pub template_scores: [f32; 3],
    pub present: bool,
}

impl FrameMatch {
    pub fn absent() -> Self {
        Self { score: 0.0, template_scores: [0.0; 3], present: false }
    }
}

/// The templates resized to one working size.
struct Prepared {
    width: u32,
    height: u32,
    /// (x, y) of every pixel in the band around the frame.
    band: Vec<(u32, u32)>,
    /// Zero-mean, unit-norm edge strength at the band points, per template.
    edges: Vec<Vec<f32>>,
}

pub struct FrameDetector {
    cfg: FrameDetectorConfig,
    /// The three frame images. Their outlines are near-identical, so which one matches best says
    /// nothing useful about rarity; only the best score over the three is used, for presence.
    templates: Vec<RgbaImage>,
    prepared: Mutex<Option<Prepared>>,
}

fn luma(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// Gradient magnitude (central differences) of an image's luma, row-major.
fn edge_strength(img: &RgbaImage) -> Vec<f32> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let l: Vec<f32> = img.pixels().map(|p| luma(&p.0)).collect();
    let mut out = vec![0f32; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let gx = l[y * w + x + 1] - l[y * w + x - 1];
            let gy = l[(y + 1) * w + x] - l[(y - 1) * w + x];
            out[y * w + x] = (gx * gx + gy * gy).sqrt();
        }
    }
    out
}

fn normalise(values: &mut [f32]) {
    let mean = values.iter().sum::<f32>() / values.len().max(1) as f32;
    values.iter_mut().for_each(|v| *v -= mean);
    let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
    values.iter_mut().for_each(|v| *v /= norm);
}

impl FrameDetector {
    /// The detector with the three frames bundled in the app.
    pub fn bundled(cfg: FrameDetectorConfig) -> Result<Self, VisionError> {
        let load = |bytes| -> Result<RgbaImage, VisionError> {
            Ok(image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.to_rgba8())
        };
        Ok(Self {
            cfg,
            templates: vec![load(SILVER_PNG)?, load(GOLD_PNG)?, load(PRISMATIC_PNG)?],
            prepared: Mutex::new(None),
        })
    }

    fn prepare(&self, width: u32, height: u32) -> Prepared {
        // Everything below this line is ignored: it is where the tooltip lands.
        let cutoff = ((height as f32 * self.cfg.top_fraction).round() as i32).clamp(1, height as i32);
        let resized: Vec<RgbaImage> =
            self.templates.iter().map(|t| imageops::resize(t, width, height, FilterType::Triangle)).collect();
        let (w, h) = (width as i32, height as i32);
        // The templates share one shape; use their union as the frame.
        let opaque_at = |x: i32, y: i32| {
            (0..w).contains(&x)
                && (0..h).contains(&y)
                && resized.iter().any(|t| t.get_pixel(x as u32, y as u32).0[3] >= OPAQUE)
        };
        let all = || (0..cutoff).flat_map(move |y| (0..w).map(move |x| (x, y)));
        // Band: every pixel within BAND of the frame, so the frame's edges on both sides count.
        let band: Vec<(u32, u32)> = all()
            .filter(|&(x, y)| (-BAND..=BAND).any(|dy| (-BAND..=BAND).any(|dx| opaque_at(x + dx, y + dy))))
            .map(|(x, y)| (x as u32, y as u32))
            .collect();

        let mut edges = Vec::new();
        for t in &resized {
            let mut composed = RgbaImage::from_pixel(width, height, CARD_INTERIOR);
            imageops::overlay(&mut composed, t, 0, 0);
            let e = edge_strength(&composed);
            let mut values: Vec<f32> = band.iter().map(|&(x, y)| e[(y * width + x) as usize]).collect();
            normalise(&mut values);
            edges.push(values);
        }
        Prepared { width, height, band, edges }
    }

    /// `region` is a capture around one card: the predicted card rectangle grown by `margin_px`
    /// on every side, so a few pixels of layout error are searched over. `card_*_px` is the
    /// predicted card size in the capture's pixels.
    pub fn detect(&self, region: &RgbaImage, card_width_px: u32, card_height_px: u32, margin_px: u32) -> FrameMatch {
        if card_height_px == 0 || region.width() < card_width_px || region.height() < card_height_px {
            return FrameMatch::absent();
        }
        // Work at a fixed card height, whatever the screen resolution.
        let scale = (WORK_HEIGHT as f32 / card_height_px as f32).min(1.0);
        let work = if scale < 1.0 {
            let w = (region.width() as f32 * scale).round() as u32;
            let h = (region.height() as f32 * scale).round() as u32;
            imageops::resize(region, w.max(1), h.max(1), FilterType::Triangle)
        } else {
            region.clone()
        };
        let tw = ((card_width_px as f32 * scale).round() as u32).max(8);
        let th = ((card_height_px as f32 * scale).round() as u32).max(8);
        let margin = (margin_px as f32 * scale).round() as u32;
        if work.width() < tw || work.height() < th {
            return FrameMatch::absent();
        }

        let mut guard = self.prepared.lock().unwrap_or_else(|p| p.into_inner());
        if guard.as_ref().is_none_or(|p| (p.width, p.height) != (tw, th)) {
            *guard = Some(self.prepare(tw, th));
        }
        let prepared = guard.as_ref().expect("prepared above");
        if prepared.band.is_empty() {
            return FrameMatch::absent();
        }

        let edges = edge_strength(&work);
        let stride = work.width() as usize;
        let max_dx = (work.width() - tw).min(2 * margin);
        let max_dy = (work.height() - th).min(2 * margin);
        let n = prepared.band.len() as f32;

        let mut best_per_template = [f32::MIN; 3];
        let mut best = f32::MIN;
        let mut samples = vec![0f32; prepared.band.len()];
        for oy in 0..=max_dy {
            for ox in 0..=max_dx {
                let (mut sum, mut sumsq) = (0f32, 0f32);
                for (s, &(x, y)) in samples.iter_mut().zip(&prepared.band) {
                    let v = edges[(y + oy) as usize * stride + (x + ox) as usize];
                    *s = v;
                    sum += v;
                    sumsq += v * v;
                }
                let var = sumsq - sum * sum / n;
                if var <= 1e-3 {
                    continue;
                }
                let (sd, mean) = (var.sqrt(), sum / n);
                for (i, template) in prepared.edges.iter().enumerate() {
                    let cross: f32 = samples.iter().zip(template).map(|(s, t)| (s - mean) * t).sum();
                    let score = cross / sd;
                    best_per_template[i] = best_per_template[i].max(score);
                    best = best.max(score);
                }
            }
        }
        if best == f32::MIN {
            return FrameMatch::absent();
        }
        let template_scores = best_per_template.map(|s| if s == f32::MIN { 0.0 } else { s });
        FrameMatch { score: best, template_scores, present: best >= self.cfg.present_threshold }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::imageops::crop_rgba;
    use mayhem_core::geometry::PixelRect;
    use mayhem_core::layout::AugmentLayout;

    /// Real in-game screenshots of one Gold offer (Scopier Weapons, Vulnerability, Combusting
    /// Interest) at five resolutions, supplied on 2026-09-29.
    pub(crate) const SCREENS: [(&str, &[u8]); 5] = [
        ("1024x768", include_bytes!("../tests/fixtures/screens/augments_1024x768_gold.webp")),
        ("1280x1024", include_bytes!("../tests/fixtures/screens/augments_1280x1024_gold.webp")),
        ("1600x900", include_bytes!("../tests/fixtures/screens/augments_1600x900_gold.webp")),
        ("1920x1080", include_bytes!("../tests/fixtures/screens/augments_1920x1080_gold.jpg")),
        ("2000x844", include_bytes!("../tests/fixtures/screens/augments_2000x844_gold.jpg")),
    ];

    /// Five full-window offers supplied on 2026-09-30 (Overflow, Ravenous Bind, Upgrade Sheen), at
    /// two aspect ratios we had never measured (16:10) and, for the first time, a native 3440×1440.
    /// In every one, **card 0's reroll is spent and the other two are unused**, so both appearances
    /// of the reroll button are covered at every resolution.
    pub(crate) const REROLL_SCREENS: [(&str, &[u8]); 5] = [
        ("1280x1024 rerolls", include_bytes!("../tests/fixtures/screens/augments_1280x1024_rerolls.png")),
        ("1440x900 rerolls", include_bytes!("../tests/fixtures/screens/augments_1440x900_rerolls.png")),
        ("1680x1050 rerolls", include_bytes!("../tests/fixtures/screens/augments_1680x1050_rerolls.png")),
        ("1920x1080 rerolls", include_bytes!("../tests/fixtures/screens/augments_1920x1080_rerolls.png")),
        ("3440x1440 rerolls", include_bytes!("../tests/fixtures/screens/augments_3440x1440_rerolls.png")),
    ];

    /// Every full-window augment screenshot we have: twelve, over nine resolutions and four aspect
    /// ratios. The desktop composite is not here because it is a crop, not a window.
    pub(crate) const ALL_OFFER_SCREENS: [(&str, &[u8]); 12] = [
        SCREENS[0],
        SCREENS[1],
        SCREENS[2],
        SCREENS[3],
        SCREENS[4],
        RARITY_SCREENS[0],
        RARITY_SCREENS[1],
        REROLL_SCREENS[0],
        REROLL_SCREENS[1],
        REROLL_SCREENS[2],
        REROLL_SCREENS[3],
        REROLL_SCREENS[4],
    ];

    /// Full-window offers of the other two rarities: a 3440×1440 game scaled to 2000×837.
    /// Prismatic: Ultimate Revolution, Blade Waltz, Overloaded (a champion portrait overlaps the
    /// third frame). Silver: Infernal Soul, Juiced, FireFox.
    pub(crate) const RARITY_SCREENS: [(&str, &[u8]); 2] = [
        ("2000x837 prismatic", include_bytes!("../tests/fixtures/screens/augments_2000x837_prismatic.webp")),
        ("2000x837 silver", include_bytes!("../tests/fixtures/screens/augments_2000x837_silver.webp")),
    ];

    /// The other two rarities are found just as well as Gold. What the frames are *not* asked any
    /// more is which rarity they are: that comes from the augment's name now.
    #[test]
    fn finds_silver_and_prismatic_offers() {
        let detector = FrameDetector::bundled(FrameDetectorConfig::default()).unwrap();
        for (name, bytes) in RARITY_SCREENS {
            let frame = load(bytes);
            for (i, m) in detect_cards(&detector, &frame, &full(&frame), 0).iter().enumerate() {
                let m = m.expect("card region inside the frame");
                // Measured with `top_fraction` 0.55: the lowest real card scores 0.545 clean and
                // 0.490 under a tooltip, while the highest empty region reaches 0.251.
                assert!(m.present && m.score > 0.45, "{name} card {i}: {m:?}");
            }
            let shift = (frame.height() as f32 * 0.17) as i32;
            for m in detect_cards(&detector, &frame, &full(&frame), shift).iter().flatten() {
                assert!(!m.present, "{name}: no card here, {m:?}");
            }
        }
    }

    /// A Prismatic offer (Symphony of War, Prom Queen, Jeweled Gauntlet) from a 3440×1440 game,
    /// in a downscaled 2000×457 screenshot of the whole desktop, cropped. The game window's client
    /// area, in this image's pixels, was worked out from the cards' size and spacing.
    const PRISMATIC_COMPOSITE: &[u8] =
        include_bytes!("../tests/fixtures/screens/augments_prismatic_3440x1440_desktop_composite.webp");
    const PRISMATIC_CLIENT: PixelRect = PixelRect { x: -81, y: -16, width: 1433, height: 600 };

    pub(crate) fn load(bytes: &[u8]) -> RgbaImage {
        image::load_from_memory(bytes).expect("fixture decodes").to_rgba8()
    }

    /// One entry per card; `None` when the (shifted) region falls outside the image.
    fn detect_cards(
        detector: &FrameDetector,
        frame: &RgbaImage,
        client: &PixelRect,
        shift_x: i32,
    ) -> Vec<Option<FrameMatch>> {
        let layout = AugmentLayout::default();
        let margin = (client.height as f32 * 0.01).round() as i32;
        layout
            .cards(client)
            .iter()
            .map(|c| {
                let grown = PixelRect::new(
                    c.x - margin + shift_x,
                    c.y - margin,
                    c.width + 2 * margin as u32,
                    c.height + 2 * margin as u32,
                );
                let region = crop_rgba(frame, &grown)?;
                Some(detector.detect(&region, c.width, c.height, margin as u32))
            })
            .collect()
    }

    fn full(frame: &RgbaImage) -> PixelRect {
        PixelRect::new(0, 0, frame.width(), frame.height())
    }

    /// The game's own augment tooltip is drawn over the lower half of the cards when it is long.
    /// Matching the whole outline then failed, the offer was treated as closed, and the panels
    /// vanished while the cards were plainly still on screen.
    #[test]
    fn cards_are_still_found_when_a_tooltip_covers_their_lower_half() {
        let detector = FrameDetector::bundled(FrameDetectorConfig::default()).unwrap();
        for (name, bytes) in SCREENS {
            let mut frame = load(bytes);
            let client = full(&frame);
            let layout = AugmentLayout::default();

            // Paint the tooltip where the real one sits: opaque dark grey from 45% of the card
            // height downwards, across the full width of the card area, measured off an in-game
            // capture of a long tooltip (2026-09-30).
            let cards = layout.cards(&client);
            let top = cards[0].y + (cards[0].height as f32 * 0.45) as i32;
            let left = cards[0].x.max(0) as u32;
            let right = ((cards[2].x + cards[2].width as i32) as u32).min(frame.width());
            let bottom = ((cards[0].y + cards[0].height as i32) as u32).min(frame.height());
            for y in (top.max(0) as u32)..bottom {
                for x in left..right {
                    frame.put_pixel(x, y, image::Rgba([20, 22, 28, 255]));
                }
            }

            for (i, m) in detect_cards(&detector, &frame, &client, 0).iter().enumerate() {
                let m = m.expect("card region inside the frame");
                assert!(m.present, "{name} card {i} lost behind the tooltip: {m:?}");
            }

            // And an empty stretch of screen must still read as empty with the shorter band.
            let shift = (frame.height() as f32 * 0.17) as i32;
            for m in detect_cards(&detector, &frame, &client, shift).iter().flatten() {
                assert!(!m.present, "{name}: no card here, {m:?}");
            }
        }
    }

    #[test]
    fn finds_all_three_gold_cards_at_every_resolution() {
        let detector = FrameDetector::bundled(FrameDetectorConfig::default()).unwrap();
        for (name, bytes) in SCREENS {
            let frame = load(bytes);
            for (i, m) in detect_cards(&detector, &frame, &full(&frame), 0).iter().enumerate() {
                let m = m.expect("card region inside the frame");
                // Measured with `top_fraction` 0.55: the lowest real card scores 0.545 clean and
                // 0.490 under a tooltip, while the highest empty region reaches 0.251. The bar sits
                // between the two, well above the 0.35 presence threshold.
                assert!(m.present && m.score > 0.45, "{name} card {i}: {m:?}");
            }
        }
    }

    #[test]
    fn finds_prismatic_cards_despite_their_glow() {
        let detector = FrameDetector::bundled(FrameDetectorConfig::default()).unwrap();
        let frame = load(PRISMATIC_COMPOSITE);
        for (i, m) in detect_cards(&detector, &frame, &PRISMATIC_CLIENT, 0).iter().enumerate() {
            let m = m.expect("card region inside the image");
            assert!(m.present, "card {i}: {m:?}");
        }
    }

    #[test]
    fn no_cards_where_there_are_none() {
        let detector = FrameDetector::bundled(FrameDetectorConfig::default()).unwrap();
        for (name, bytes) in SCREENS {
            let frame = load(bytes);
            // Half a card spacing to the side: the regions straddle the gaps between cards.
            let shift = (frame.height() as f32 * 0.17) as i32;
            for m in detect_cards(&detector, &frame, &full(&frame), shift).iter().flatten() {
                assert!(!m.present && m.score < 0.25, "{name}: {m:?}");
            }
        }
        // A flat image has no frame at all.
        let flat = RgbaImage::from_pixel(400, 600, Rgba([40, 60, 70, 255]));
        assert!(!detector.detect(&flat, 300, 500, 10).present);
    }
}
