//! Text recognition with the bundled PaddleOCR PP-OCRv4 model, run in pure Rust by `tract`.
//!
//! Self-contained by design: the model ships inside the app, so nothing has to be
//! installed on the user's machine — no Windows OCR language pack, no native runtime. Only the
//! recognition half of PaddleOCR is used, since the card layout already says where each title is.
//!
//! Measured on the title crops of six real screenshots (five resolutions, and a downscaled
//! Prismatic offer): every letter of all 18 titles read correctly. Spaces are sometimes dropped
//! ("ScopierWeapons"), which the name matcher ignores.
//!
//! One card per run, three runs in parallel. The model was first built for a batch of three, which
//! `tract` optimised far worse than a batch of one: 763 ms for the three titles, against 87 ms for
//! a single card and 90 ms for all three on their own threads — an eight-fold saving on every OCR
//! pass, and the reason a rerolled augment now refreshes in well under a second.

use image::imageops::{self, FilterType};
use image::RgbaImage;
use tract_onnx::prelude::*;

use crate::ocr::{OcrEngine, OcrText};
use crate::VisionError;

const MODEL: &[u8] = include_bytes!("../assets/ocr/ppocrv4_rec_ch.onnx");
const DICT: &str = include_str!("../assets/ocr/ppocrv4_rec_ch.dict.txt");

/// The model's input height.
const HEIGHT: usize = 48;
/// Fixed input width. Title crops are ~8.6:1 (0.25H × 0.029H), i.e. ~413 px at 48 high; wider
/// crops are squeezed to fit, narrower ones padded.
const WIDTH: usize = 448;
/// Crops per model run. One card at a time: the three titles are recognised on three threads, and
/// a reroll changes a single card, which then costs one run instead of a batch of three.
const BATCH: usize = 1;

type Model = std::sync::Arc<TypedRunnableModel>;

pub struct PaddleRecognizer {
    model: Model,
    /// Index 0 is the CTC blank; then the dictionary; then a space.
    chars: Vec<String>,
}

fn err(e: impl std::fmt::Display) -> VisionError {
    VisionError::OcrUnavailable(e.to_string())
}

impl PaddleRecognizer {
    /// Loads the bundled model (~0.2 s in release builds). Create once and reuse.
    pub fn bundled() -> Result<Self, VisionError> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(MODEL))
            .and_then(|m| m.with_input_fact(0, f32::fact([BATCH, 3, HEIGHT, WIDTH]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(err)?;
        let mut chars = vec![String::new()];
        // `lines`, not `split('\n')`: a CRLF checkout (GitHub's Windows runners) would otherwise
        // leave a `\r` on every character and every read.
        chars.extend(DICT.lines().map(str::to_owned));
        chars.push(" ".into());
        Ok(Self { model, chars })
    }

    /// One crop as the model's input: resized to the model's height keeping the aspect ratio
    /// (squeezed if too wide) and normalised to [-1, 1]; the padding stays at 0 like PaddleOCR's.
    fn input_for(crop: &RgbaImage) -> tract_ndarray::Array4<f32> {
        let mut input = tract_ndarray::Array4::<f32>::zeros((BATCH, 3, HEIGHT, WIDTH));
        if crop.width() == 0 || crop.height() == 0 {
            return input;
        }
        let w = ((crop.width() as f32 * HEIGHT as f32 / crop.height() as f32).round() as usize).clamp(1, WIDTH);
        let resized = imageops::resize(crop, w as u32, HEIGHT as u32, FilterType::Triangle);
        for (x, y, p) in resized.enumerate_pixels() {
            for c in 0..3 {
                input[[0, c, y as usize, x as usize]] = (p.0[c] as f32 / 255.0 - 0.5) / 0.5;
            }
        }
        input
    }

    /// Recognise one crop. Takes `&self` only, so several can run at once on different threads.
    fn run_one(&self, crop: &RgbaImage) -> Result<OcrText, VisionError> {
        let input = Self::input_for(crop);
        let result = self
            .model
            .run(tvec!(Tensor::from(input).into()))
            .map_err(|e: TractError| VisionError::Ocr(e.to_string()))?;
        let probs = result[0].clone().into_tensor();
        let probs = probs.to_plain_array_view::<f32>().map_err(|e: TractError| VisionError::Ocr(e.to_string()))?;
        let probs = probs.into_dimensionality::<tract_ndarray::Ix3>().map_err(|e| VisionError::Ocr(e.to_string()))?;
        Ok(self.decode(probs.index_axis(tract_ndarray::Axis(0), 0)))
    }

    /// Greedy CTC decoding: best class per time step, collapse repeats, drop blanks.
    fn decode(&self, probs: tract_ndarray::ArrayView2<f32>) -> OcrText {
        let mut text = String::new();
        let mut last = 0usize;
        for row in probs.rows() {
            let (best, _) =
                row.iter().enumerate().fold((0usize, f32::MIN), |acc, (i, &p)| if p > acc.1 { (i, p) } else { acc });
            if best != last && best != 0 {
                if let Some(c) = self.chars.get(best) {
                    text.push_str(c);
                }
            }
            last = best;
        }
        OcrText { lines: vec![text.trim().to_owned()] }
    }
}

impl OcrEngine for PaddleRecognizer {
    fn describe(&self) -> String {
        "PaddleOCR PP-OCRv4 (bundled)".into()
    }

    fn recognize(&self, image: &RgbaImage) -> Result<OcrText, VisionError> {
        self.run_one(image)
    }

    /// One thread per crop. A single run leaves most of the machine idle, and the three titles are
    /// independent, so recognising all three costs about as long as recognising one.
    fn recognize_all(&self, images: &[RgbaImage]) -> Result<Vec<OcrText>, VisionError> {
        if images.len() < 2 {
            return images.iter().map(|i| self.run_one(i)).collect();
        }
        std::thread::scope(|scope| {
            let running: Vec<_> = images.iter().map(|image| scope.spawn(move || self.run_one(image))).collect();
            running
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|_| Err(VisionError::Ocr("the OCR thread panicked".into()))))
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{ScreenCapture, StillCapture};
    use crate::frames::tests::{load, SCREENS};
    use crate::matcher::{AugmentMatcher, AugmentName, MatcherConfig};
    use crate::offer::CardDetectors;
    use crate::offer::{read_offer, scan_cards};
    use mayhem_core::layout::{AugmentLayout, TitleLines};

    fn vocabulary() -> AugmentMatcher {
        let n = |id, name: &str, rarity: &str| AugmentName { id, name: name.into(), rarity: rarity.into() };
        AugmentMatcher::new(
            MatcherConfig::default(),
            &[
                n(1, "Scopier Weapons", "kGold"),
                n(2, "Vulnerability", "kGold"),
                n(3, "Combusting Interest", "kGold"),
                n(4, "Symphony of War", "kPrismatic"),
                n(5, "Prom Queen", "kPrismatic"),
                n(6, "Jeweled Gauntlet", "kPrismatic"),
                n(10, "Ultimate Revolution", "kPrismatic"),
                n(11, "Blade Waltz", "kPrismatic"),
                n(12, "Overloaded", "kPrismatic"),
                n(13, "Infernal Soul", "kSilver"),
                n(14, "Juiced", "kSilver"),
                n(15, "FireFox", "kSilver"),
                n(20, "Overflow", "kGold"),
                n(21, "Ravenous Bind", "kGold"),
                n(22, "Upgrade Sheen", "kGold"),
                // Distractors with overlapping words.
                n(16, "Ultimate Unstoppable", "kGold"),
                n(17, "Infernal Conduit", "kGold"),
                n(7, "Scoped Weapons", "kSilver"),
                n(8, "Combustion", "kGold"),
                n(9, "War Queen", "kPrismatic"),
                n(23, "Overload", "kSilver"),
                n(24, "Upgrade Spellblade", "kGold"),
                n(25, "Ravenous Hunter", "kPrismatic"),
            ],
        )
    }

    /// The whole pipeline on real screenshots: layout → frames → title crops → bundled OCR →
    /// matcher. No scripted text anywhere.
    #[test]
    fn reads_real_gold_offers_at_every_resolution() {
        let ocr = PaddleRecognizer::bundled().unwrap();
        let detectors = CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap();
        let matcher = vocabulary();
        for (name, bytes) in SCREENS {
            let mut cap = StillCapture { frame: load(bytes) };
            let client = cap.game_client_rect().unwrap();
            let scan = scan_cards(&mut cap, &client, &AugmentLayout::default(), &detectors, false).unwrap();
            let titles = scan.titles.clone().expect("cards on screen");
            let read = read_offer(&ocr, &matcher, &titles, AugmentLayout::default().title_lines()).unwrap();
            assert_eq!(read.reading.slots, [Some(1), Some(2), Some(3)], "{name}: read {:?}", read.texts);
        }
    }

    #[test]
    fn reads_real_silver_and_prismatic_offers() {
        use crate::frames::tests::RARITY_SCREENS;
        let ocr = PaddleRecognizer::bundled().unwrap();
        let detectors = CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap();
        let matcher = vocabulary();
        let expected = [[Some(10), Some(11), Some(12)], [Some(13), Some(14), Some(15)]];
        for ((name, bytes), want) in RARITY_SCREENS.iter().zip(expected) {
            let mut cap = StillCapture { frame: load(bytes) };
            let client = cap.game_client_rect().unwrap();
            let scan = scan_cards(&mut cap, &client, &AugmentLayout::default(), &detectors, false).unwrap();
            let titles = scan.titles.clone().expect("cards on screen");
            let read = read_offer(&ocr, &matcher, &titles, AugmentLayout::default().title_lines()).unwrap();
            assert_eq!(read.reading.slots, want, "{name}: read {:?}", read.texts);
        }
    }

    /// The five screenshots of 2026-09-30, which brought two aspect ratios we had never tested
    /// (16:10) and the first *native* 3440x1440 capture. The whole pipeline runs: reroll gate,
    /// card area, title crops, bundled OCR, matcher.
    #[test]
    fn reads_real_offers_at_the_new_resolutions() {
        use crate::frames::tests::REROLL_SCREENS;
        let ocr = PaddleRecognizer::bundled().unwrap();
        let detectors = CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap();
        let matcher = vocabulary();
        for (name, bytes) in REROLL_SCREENS {
            let mut cap = StillCapture { frame: load(bytes) };
            let client = cap.game_client_rect().unwrap();
            let scan = scan_cards(&mut cap, &client, &AugmentLayout::default(), &detectors, false).unwrap();
            assert!(scan.rerolls.iter().any(|r| r.present), "{name}: the reroll gate did not open");
            let titles = scan.titles.clone().expect("cards on screen");
            let read = read_offer(&ocr, &matcher, &titles, AugmentLayout::default().title_lines()).unwrap();
            assert_eq!(read.reading.slots, [Some(20), Some(21), Some(22)], "{name}: read {:?}", read.texts);
        }
    }

    /// "Expertise en omnivampirisme" wraps onto two lines, between two one-line titles, in the
    /// three screenshots it was reported with (2026-10-01). The recogniser reads one line, so the
    /// title used to come back as "omnivampirisme" alone and match nothing.
    #[test]
    fn reads_a_title_wrapped_onto_two_lines() {
        const WRAPPED: [(&str, &[u8]); 3] = [
            ("1024x768", include_bytes!("../tests/fixtures/screens/augments_1024x768_wrapped_fr.png")),
            ("1920x1200", include_bytes!("../tests/fixtures/screens/augments_1920x1200_wrapped_fr.png")),
            ("3424x1401", include_bytes!("../tests/fixtures/screens/augments_3424x1401_wrapped_fr.png")),
        ];
        let n = |id, name: &str| AugmentName { id, name: name.into(), rarity: "kSilver".into() };
        let matcher = AugmentMatcher::new(
            MatcherConfig::default(),
            &[
                n(1, "Gros cerveau"),
                n(2, "Expertise en omnivampirisme"),
                n(3, "Machine à rétrécir"),
                // Real pool names that share words with the wrapped title. "Vampirisme" is inside
                // its second line, which is all that used to be read.
                n(4, "Vampirisme"),
                n(5, "Expertise en critiques"),
                n(6, "Gros calibre"),
            ],
        );
        let ocr = PaddleRecognizer::bundled().unwrap();
        let detectors = CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap();
        let layout = AugmentLayout::default();
        for (name, bytes) in WRAPPED {
            let mut cap = StillCapture { frame: load(bytes) };
            let client = cap.game_client_rect().unwrap();
            let scan = scan_cards(&mut cap, &client, &layout, &detectors, false).unwrap();
            let titles = scan.titles.clone().expect("cards on screen");
            let cut = titles.each_ref().map(|t| crate::offer::title_line_crops(t, layout.title_lines()).len());
            assert_eq!(cut, [1, 2, 1], "{name}: lines per card");
            let read = read_offer(&ocr, &matcher, &titles, layout.title_lines()).unwrap();
            assert_eq!(read.reading.slots, [Some(1), Some(2), Some(3)], "{name}: read {:?}", read.texts);
        }
    }

    #[test]
    fn reads_the_prismatic_offer_from_a_downscaled_desktop_screenshot() {
        use crate::imageops::crop_rgba;
        use mayhem_core::geometry::PixelRect;
        let ocr = PaddleRecognizer::bundled().unwrap();
        let frame =
            load(include_bytes!("../tests/fixtures/screens/augments_prismatic_3440x1440_desktop_composite.webp"));
        // Client area in this image (see frames.rs); only the title bands are needed here, and a
        // band on its own is a block of one line.
        let client = PixelRect { x: -81, y: -16, width: 1433, height: 600 };
        let titles = AugmentLayout::default().titles(&client).map(|t| crop_rgba(&frame, &t).expect("inside the image"));
        let read = read_offer(&ocr, &vocabulary(), &titles, TitleLines::SINGLE).unwrap();
        assert_eq!(read.reading.slots, [Some(4), Some(5), Some(6)], "read {:?}", read.texts);
    }
}
