//! Cuts the "hide augments" button template out of a real screenshot and writes it next to the
//! fixtures, so the bundled asset is a crop of the game rather than a redrawing of it.
//!
//! `cargo test --release -p mayhem-vision --test augment_button_template -- --ignored --nocapture`

use image::RgbaImage;

const SOURCE: &[u8] = include_bytes!("fixtures/screens/augments_1920x1080_gold.jpg");

/// The teal plate measured by `augment_button_measure` on this screenshot, in its own pixels.
const PLATE: (u32, u32, u32, u32) = (866, 833, 191, 48);

#[test]
#[ignore = "asset tool"]
fn cut_the_button_template() {
    let img: RgbaImage = image::load_from_memory(SOURCE).unwrap().to_rgba8();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/button-crops");
    std::fs::create_dir_all(&out).unwrap();
    // Several pads, so the one that captures the button's own furniture without dragging in the
    // moving game world behind it can be picked by eye.
    for pad in [0u32, 4, 8, 12] {
        let (x, y, w, h) = PLATE;
        let (x, y) = (x - pad, y - pad);
        let (w, h) = (w + 2 * pad, h + 2 * pad);
        let crop = image::imageops::crop_imm(&img, x, y, w, h).to_image();
        let path = out.join(format!("button_pad{pad}.png"));
        crop.save(&path).unwrap();
        println!("pad {pad}: {w}x{h} at ({x},{y}) -> {}", path.display());
    }
}
