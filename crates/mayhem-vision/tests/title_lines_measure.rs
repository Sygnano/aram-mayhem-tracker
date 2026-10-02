//! Measurement tool for wrapped titles: how each card's title block is cut into lines, where the
//! text in it actually is, and what OCR reads off each line.
//!
//! `cargo test --release -p mayhem-vision --test title_lines_measure -- --ignored --nocapture`

use image::RgbaImage;
use mayhem_core::geometry::PixelRect;
use mayhem_core::layout::AugmentLayout;
use mayhem_vision::capture::StillCapture;
use mayhem_vision::imageops::to_gray;
use mayhem_vision::ocr::OcrEngine;
use mayhem_vision::offer::{scan_cards, title_line_crops, CardDetectors};
use mayhem_vision::paddle::PaddleRecognizer;

const SCREENS: [(&str, &[u8]); 3] = [
    ("1024x768 wrapped", include_bytes!("fixtures/screens/augments_1024x768_wrapped_fr.png")),
    ("1920x1200 wrapped", include_bytes!("fixtures/screens/augments_1920x1200_wrapped_fr.png")),
    ("3424x1401 wrapped", include_bytes!("fixtures/screens/augments_3424x1401_wrapped_fr.png")),
];

/// Runs of rows holding text-bright pixels, as fractions of the client height.
fn text_rows(block: &RgbaImage, top: i32, client_height: u32) -> Vec<(f32, f32)> {
    let gray = to_gray(block);
    let lit = |y: u32| (0..gray.width()).any(|x| gray.get_pixel(x, y).0[0] > 170);
    let h = client_height as f32;
    let mut runs = Vec::new();
    let mut start = None;
    for y in 0..=gray.height() {
        match (start, y < gray.height() && lit(y)) {
            (None, true) => start = Some(y),
            (Some(s), false) => {
                runs.push(((top + s as i32) as f32 / h, (top + y as i32) as f32 / h));
                start = None;
            }
            _ => {}
        }
    }
    runs
}

#[test]
#[ignore = "measurement tool"]
fn measure_title_lines() {
    let layout = AugmentLayout::default();
    let lines = layout.title_lines();
    let detectors = CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap();
    let ocr = PaddleRecognizer::bundled().unwrap();

    for (name, bytes) in SCREENS {
        let frame: RgbaImage = image::load_from_memory(bytes).unwrap().to_rgba8();
        let client = PixelRect::new(0, 0, frame.width(), frame.height());
        let mut capture = StillCapture { frame };
        let scan = scan_cards(&mut capture, &client, &layout, &detectors, true).unwrap();
        println!("== {name}");
        let Some(titles) = scan.titles else {
            println!("   no cards found");
            continue;
        };
        for (i, block) in titles.iter().enumerate() {
            let title = layout.title(&client, i);
            let block_top = title.y - lines.rise(title.height) as i32;
            let crops = title_line_crops(block, lines);
            let texts: Vec<String> = ocr.recognize_all(&crops).unwrap().iter().map(|t| t.joined()).collect();
            println!("   card {i}: {} line(s) {texts:?}", crops.len());
            println!("      text rows {:.4?}", text_rows(block, block_top, client.height));
        }
    }
}
