//! Measurement tool: locates the in-game "hide augments" button in the real screenshots, so its
//! layout constants are built from numbers rather than from eyeballing (the same method as
//! `champ_select_measure`).
//!
//! The button is the one piece of the augment screen that never changes: a fixed teal plate with a
//! stack-of-cards glyph, present exactly while augments can be selected, and unaffected by the
//! rarity glow that makes the card frames hard to find. Its bright cyan border is unique in the
//! lower middle of the screen, so a hue/saturation filter finds it without a template.
//!
//! `cargo test --release -p mayhem-vision --test augment_button_measure -- --ignored --nocapture`

use image::RgbaImage;

const SCREENS: [(&str, &[u8]); 7] = [
    ("1024x768 gold", include_bytes!("fixtures/screens/augments_1024x768_gold.webp")),
    ("1280x1024 gold", include_bytes!("fixtures/screens/augments_1280x1024_gold.webp")),
    ("1600x900 gold", include_bytes!("fixtures/screens/augments_1600x900_gold.webp")),
    ("1920x1080 gold", include_bytes!("fixtures/screens/augments_1920x1080_gold.jpg")),
    ("2000x844 gold", include_bytes!("fixtures/screens/augments_2000x844_gold.jpg")),
    ("2000x837 prismatic", include_bytes!("fixtures/screens/augments_2000x837_prismatic.webp")),
    ("2000x837 silver", include_bytes!("fixtures/screens/augments_2000x837_silver.webp")),
];

fn load(bytes: &[u8]) -> RgbaImage {
    image::load_from_memory(bytes).unwrap().to_rgba8()
}

fn hsv(p: &[u8]) -> (f32, f32, f32) {
    let (r, g, b) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;
    let hue = if chroma < 1e-6 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / chroma) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / chroma + 2.0)
    } else {
        60.0 * ((r - g) / chroma + 4.0)
    };
    (if hue < 0.0 { hue + 360.0 } else { hue }, if max <= 1e-6 { 0.0 } else { chroma / max }, max)
}

/// The button's teal: measured by widening this band until the plate came out as one solid block.
fn is_button_teal(p: &[u8]) -> bool {
    let (hue, sat, value) = hsv(p);
    (168.0..205.0).contains(&hue) && sat > 0.45 && value > 0.45
}

#[test]
#[ignore = "measurement tool"]
fn measure_the_hide_augments_button() {
    println!("button box in units of client height H, x relative to the client centre\n");
    for (name, bytes) in SCREENS {
        let img = load(bytes);
        let (w, h) = (img.width() as f32, img.height() as f32);
        let centre = w / 2.0;
        // Search the lower middle only: the button sits above the level-up row, and the rest of the
        // HUD carries teal of its own.
        let (y0, y1) = ((0.68 * h) as u32, (0.88 * h).min(h) as u32);
        let (x0, x1) = ((centre - 0.30 * h).max(0.0) as u32, (centre + 0.30 * h).min(w) as u32);

        let count = |xs: std::ops::Range<u32>, ys: std::ops::Range<u32>, along_x: bool| -> Vec<u32> {
            let outer: Vec<u32> = if along_x { xs.clone().collect() } else { ys.clone().collect() };
            outer
                .into_iter()
                .map(|o| {
                    let inner = if along_x { ys.clone() } else { xs.clone() };
                    inner
                        .filter(|&i| {
                            let (x, y) = if along_x { (o, i) } else { (i, o) };
                            is_button_teal(&img.get_pixel(x, y).0)
                        })
                        .count() as u32
                })
                .collect()
        };
        // Half the peak: the plate is solid teal across its full width and height, so its own rows
        // and columns sit near the peak while stray HUD teal stays far below it.
        let span = |counts: &[u32], base: u32| -> Option<(u32, u32)> {
            let floor = (counts.iter().copied().max()? / 2).max(2);
            let first = counts.iter().position(|&c| c >= floor)?;
            let last = counts.iter().rposition(|&c| c >= floor)?;
            Some((base + first as u32, base + last as u32))
        };

        // Columns first, over the whole search window; then rows, but only within the columns the
        // plate actually occupies, so the HUD's own teal below it cannot stretch the box.
        let Some((left, right)) = span(&count(x0..x1, y0..y1, true), x0) else {
            println!("{name}: no button found");
            continue;
        };
        let Some((top, bottom)) = span(&count(left..right + 1, y0..y1, false), y0) else {
            println!("{name}: no button rows found");
            continue;
        };

        let (bw, bh) = ((right - left + 1) as f32, (bottom - top + 1) as f32);
        println!(
            "{name} ({}x{}):\n  px  left {left} right {right} top {top} bottom {bottom}  ({bw}x{bh})\n  \
             H   cx {:+.4}  top {:.4}  width {:.4}  height {:.4}  aspect {:.3}",
            img.width(),
            img.height(),
            ((left as f32 + right as f32) / 2.0 - centre) / h,
            (top as f32) / h,
            bw / h,
            bh / h,
            bw / bh,
        );
    }
}
