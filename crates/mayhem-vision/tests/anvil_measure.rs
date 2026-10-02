//! Measurement tool for stat anvil offers: what the augment detectors score on anvil cards,
//! and what OCR reads off the name and value lines at the positions measured in the design.
//!
//! `cargo test --release -p mayhem-vision --test anvil_measure -- --ignored --nocapture`

use image::RgbaImage;
use mayhem_core::geometry::PixelRect;
use mayhem_core::layout::AugmentLayout;
use mayhem_vision::capture::StillCapture;
use mayhem_vision::imageops::crop_rgba;
use mayhem_vision::ocr::OcrEngine;
use mayhem_vision::offer::{scan_cards, CardDetectors};
use mayhem_vision::paddle::PaddleRecognizer;

const ANVIL_SCREENS: [(&str, &[u8]); 7] = [
    ("1280x1024 gold", include_bytes!("fixtures/screens/anvil_1280x1024_gold.png")),
    ("1440x900 gold", include_bytes!("fixtures/screens/anvil_1440x900_gold.png")),
    ("1680x1050 gold", include_bytes!("fixtures/screens/anvil_1680x1050_gold.png")),
    ("1920x1080 gold", include_bytes!("fixtures/screens/anvil_1920x1080_gold.png")),
    ("3440x1440 silver", include_bytes!("fixtures/screens/anvil_3440x1440_silver.png")),
    ("3440x1440 gold", include_bytes!("fixtures/screens/anvil_3440x1440_gold.png")),
    ("3440x1440 prismatic", include_bytes!("fixtures/screens/anvil_3440x1440_prismatic.png")),
];

fn band(client: &PixelRect, cx: f32, top: f32, half_width: f32, height: f32) -> PixelRect {
    let h = client.height as f32;
    let centre = client.width as f32 / 2.0;
    PixelRect::new(
        (centre + (cx - half_width) * h).round() as i32,
        (top * h).round() as i32,
        (2.0 * half_width * h).round() as u32,
        (height * h).round() as u32,
    )
}

#[test]
#[ignore]
fn measure_detectors_and_ocr_on_anvil_cards() {
    let layout = AugmentLayout::default();
    let detectors = CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap();
    let ocr = PaddleRecognizer::bundled().unwrap();

    for (name, bytes) in ANVIL_SCREENS {
        let frame: RgbaImage = image::load_from_memory(bytes).unwrap().to_rgba8();
        let client = PixelRect::new(0, 0, frame.width(), frame.height());
        let mut capture = StillCapture { frame: frame.clone() };
        let scan = scan_cards(&mut capture, &client, &layout, &detectors, true).unwrap();
        println!("== {name}");
        println!(
            "   rerolls {:?}  button {:.3} present={}  frames {:?}",
            scan.rerolls.map(|r| (r.score * 1000.0).round() / 1000.0),
            scan.button.score,
            scan.button.present,
            scan.frames.map(|f| ((f.score * 1000.0).round() / 1000.0, f.present)),
        );

        for i in 0..3 {
            let cx = layout.card_centers[i];
            let crops = [
                layout.title(&client, i),
                band(&client, cx, 0.465, 0.110, 0.022),
                band(&client, cx, 0.497, 0.110, 0.022),
            ];
            let images: Vec<RgbaImage> = crops.iter().map(|r| crop_rgba(&frame, r).unwrap()).collect();
            let texts = ocr.recognize_all(&images).unwrap();
            println!(
                "   card {i}: name {:?} | value1 {:?} | value2 {:?}",
                texts[0].joined(),
                texts[1].joined(),
                texts[2].joined()
            );
        }
    }
}
