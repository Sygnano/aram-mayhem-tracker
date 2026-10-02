//! Measurement tool for the reroll buttons under each augment card, and for the template crops.
//!
//! The reroll buttons are the sturdiest thing on the augment screen: they cannot be covered by the
//! game's own augment tooltip (which is what defeats the "hide augments" button), they have exactly
//! two appearances — unused and spent — and they vanish together with the cards.
//!
//! `cargo test --release -p mayhem-vision --test reroll_measure -- --ignored --nocapture`

use image::RgbaImage;
use mayhem_core::geometry::PixelRect;
use mayhem_core::layout::AugmentLayout;
use mayhem_vision::imageops::crop_rgba;
use mayhem_vision::reroll::{RerollDetector, RerollDetectorConfig};

/// The older augment screenshots, whose rerolls are all unused. Together with `REROLL_SCREENS` they
/// are every full-window offer we have.
const OLD_SCREENS: [(&str, &[u8]); 7] = [
    ("1024x768", include_bytes!("fixtures/screens/augments_1024x768_gold.webp")),
    ("1280x1024 gold", include_bytes!("fixtures/screens/augments_1280x1024_gold.webp")),
    ("1600x900", include_bytes!("fixtures/screens/augments_1600x900_gold.webp")),
    ("1920x1080 gold", include_bytes!("fixtures/screens/augments_1920x1080_gold.jpg")),
    ("2000x844", include_bytes!("fixtures/screens/augments_2000x844_gold.jpg")),
    ("2000x837 prismatic", include_bytes!("fixtures/screens/augments_2000x837_prismatic.webp")),
    ("2000x837 silver", include_bytes!("fixtures/screens/augments_2000x837_silver.webp")),
];

/// The five screenshots supplied on 2026-09-30, each with card 0's reroll **spent** and the other
/// two **unused**, so both appearances are measured at every resolution.
const REROLL_SCREENS: [(&str, &[u8]); 5] = [
    ("1280x1024", include_bytes!("fixtures/screens/augments_1280x1024_rerolls.png")),
    ("1440x900", include_bytes!("fixtures/screens/augments_1440x900_rerolls.png")),
    ("1680x1050", include_bytes!("fixtures/screens/augments_1680x1050_rerolls.png")),
    ("1920x1080", include_bytes!("fixtures/screens/augments_1920x1080_rerolls.png")),
    ("3440x1440", include_bytes!("fixtures/screens/augments_3440x1440_rerolls.png")),
];

fn load(bytes: &[u8]) -> RgbaImage {
    image::load_from_memory(bytes).unwrap().to_rgba8()
}

fn luma(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// A generous box around where card `i`'s reroll button should be, to search inside.
fn search_box(layout: &AugmentLayout, client: &PixelRect, i: usize) -> PixelRect {
    let h = client.height as f32;
    let centre = client.x as f32 + client.width as f32 / 2.0;
    let cx = centre + layout.card_centers[i] * h;
    let top = client.y as f32 + layout.reroll_top * h;
    PixelRect::new((cx - 0.07 * h) as i32, (top - 0.015 * h) as i32, (0.14 * h) as u32, (0.075 * h) as u32)
}

/// The bounding box of the brightest connected blob in `crop`, which is the button's plate: it is a
/// solid block clearly brighter than the dark backdrop in **both** states — grey when the reroll is
/// spent, gold when it is not — so one threshold finds either without knowing which it is.
///
/// A mean-luma profile does not work here: the backdrop is the game world, which is bright in
/// places, so the profile's edges land on scenery rather than on the button.
fn brightest_blob(crop: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (crop.width() as usize, crop.height() as usize);
    let l: Vec<f32> = crop.pixels().map(|p| luma(&p.0)).collect();
    let mut sorted = l.clone();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let top = sorted[sorted.len() * 98 / 100];
    let level = median + (top - median) * 0.45;

    // Flood fill each unvisited bright pixel; keep the largest component.
    let mut seen = vec![false; w * h];
    let mut best: Option<(usize, (u32, u32, u32, u32))> = None;
    for start in 0..w * h {
        if seen[start] || l[start] < level {
            continue;
        }
        let (mut stack, mut size) = (vec![start], 0usize);
        let (mut x0, mut x1, mut y0, mut y1) = (w, 0usize, h, 0usize);
        seen[start] = true;
        while let Some(p) = stack.pop() {
            let (x, y) = (p % w, p / w);
            size += 1;
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            let mut push = |q: usize, stack: &mut Vec<usize>| {
                if !seen[q] && l[q] >= level {
                    seen[q] = true;
                    stack.push(q);
                }
            };
            if x > 0 {
                push(p - 1, &mut stack)
            }
            if x + 1 < w {
                push(p + 1, &mut stack)
            }
            if y > 0 {
                push(p - w, &mut stack)
            }
            if y + 1 < h {
                push(p + w, &mut stack)
            }
        }
        if best.is_none_or(|(s, _)| size > s) {
            best = Some((size, (x0 as u32, y0 as u32, x1 as u32, y1 as u32)));
        }
    }
    best.map(|(_, b)| b)
}

#[test]
#[ignore = "measurement tool"]
fn measure_the_reroll_buttons() {
    let layout = AugmentLayout::default();
    println!("reroll button boxes, in units of client height H, x relative to the client centre\n");
    for (name, bytes) in REROLL_SCREENS {
        let img = load(bytes);
        let client = PixelRect::new(0, 0, img.width(), img.height());
        let (w, h) = (img.width() as f32, img.height() as f32);
        println!("{name}:");
        for i in 0..3 {
            let b = search_box(&layout, &client, i);
            let crop = image::imageops::crop_imm(&img, b.x as u32, b.y as u32, b.width, b.height).to_image();
            let Some((cx0, cy0, cx1, cy1)) = brightest_blob(&crop) else {
                println!("  card {i}: not found");
                continue;
            };
            let (left, right) = (b.x as u32 + cx0, b.x as u32 + cx1);
            let (top, bottom) = (b.y as u32 + cy0, b.y as u32 + cy1);
            let (bw, bh) = ((right - left + 1) as f32, (bottom - top + 1) as f32);
            println!(
                "  card {i}: px {left}..{right} x {top}..{bottom} ({bw}x{bh})  \
                 H cx {:+.4} top {:.4} w {:.4} h {:.4} aspect {:.2}",
                ((left as f32 + right as f32) / 2.0 - w / 2.0) / h,
                top as f32 / h,
                bw / h,
                bh / h,
                bw / bh,
            );
        }
    }
}

/// Cuts the two reroll templates out of the 3440x1440 screenshot — the highest resolution we have,
/// and lossless PNG — and writes them where the assets live.
///
/// Card 1's reroll is unused and card 0's is spent, in the same place relative to their cards, so
/// one rectangle cuts both states.
#[test]
#[ignore = "asset tool"]
fn cut_the_reroll_templates() {
    let img = load(REROLL_SCREENS[4].1);
    let layout = AugmentLayout::default();
    let client = PixelRect::new(0, 0, img.width(), img.height());
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/reroll");
    std::fs::create_dir_all(&out).unwrap();
    for (card, state) in [(1usize, "unused"), (0, "spent")] {
        let b = layout.reroll(&client, card);
        let crop = image::imageops::crop_imm(&img, b.x as u32, b.y as u32, b.width, b.height).to_image();
        let path = out.join(format!("{state}.png"));
        crop.save(&path).unwrap();
        println!("{state}: {}x{} at ({},{}) -> {}", b.width, b.height, b.x, b.y, path.display());
    }
}

/// Sweeps candidate template boxes and reports, for each, the worst real reroll and the best false
/// one over every screenshot we have. The box is what decides the detector's margin, because the
/// template's footprint *is* the layout box: crop too wide and the template carries background,
/// which differs from screenshot to screenshot and drowns out a dim spent plate; crop too tight and
/// the plate's own border — the strongest structure it has — is cut away.
///
/// `cargo test --release -p mayhem-vision --test reroll_measure -- --ignored --nocapture sweep`
#[test]
#[ignore = "measurement tool"]
fn sweep_reroll_template_boxes() {
    let source = load(REROLL_SCREENS[4].1);
    let src_client = PixelRect::new(0, 0, source.width(), source.height());
    let mut screens: Vec<(&str, RgbaImage)> = OLD_SCREENS.iter().map(|(n, b)| (*n, load(b))).collect();
    screens.extend(REROLL_SCREENS.iter().map(|(n, b)| (*n, load(b))));

    println!("box (H units)                    worst real   best false   gap");
    let mut best = (f32::MIN, (0f32, 0f32, 0f32));
    let combos: Vec<(f32, f32, f32)> = [0.0500f32, 0.0540, 0.0570, 0.0610]
        .into_iter()
        .flat_map(|w| {
            [0.0320f32, 0.0360, 0.0400]
                .into_iter()
                .flat_map(move |h| [0.6980f32, 0.7042, 0.7100].into_iter().map(move |cy| (w, h, cy)))
        })
        .collect();
    for (bw, bh, centre_y) in combos {
        let layout = AugmentLayout {
            reroll_box_width: bw,
            reroll_box_height: bh,
            reroll_box_top: centre_y - bh / 2.0,
            ..Default::default()
        };

        let cut = |card: usize| -> RgbaImage {
            let r = layout.reroll(&src_client, card);
            crop_rgba(&source, &r).expect("template box inside the source")
        };
        let detector = RerollDetector::new(RerollDetectorConfig::default(), [cut(1), cut(0)]);

        let margin_of = |h: u32| (h as f32 * 0.008).round() as i32;
        let score_at = |frame: &RgbaImage, i: usize, shift: i32| -> Option<f32> {
            let client = PixelRect::new(0, 0, frame.width(), frame.height());
            let b = layout.reroll(&client, i);
            let m = margin_of(client.height);
            let grown = PixelRect::new(b.x - m, b.y - m + shift, b.width + 2 * m as u32, b.height + 2 * m as u32);
            let region = crop_rgba(frame, &grown)?;
            Some(detector.detect(&region, b.width, b.height).score)
        };

        let (mut worst_real, mut best_false) = (f32::MAX, 0f32);
        // What production actually tests: the best of the three, because one reroll found is all
        // three found. A spent button scoring badly costs nothing while a sibling is unused.
        let (mut worst_row, mut row_where) = (f32::MAX, String::new());
        let (mut worst_where, mut false_where) = (String::new(), String::new());
        for (name, frame) in &screens {
            let mut row = 0f32;
            for i in 0..3 {
                if let Some(s) = score_at(frame, i, 0) {
                    row = row.max(s);
                    if s < worst_real {
                        worst_real = s;
                        worst_where = format!("{name} #{i}");
                    }
                }
            }
            if row < worst_row {
                worst_row = row;
                row_where = name.to_string();
            }
            // False positives properly: slide the box over the whole augment screen on a grid,
            // scoring one placement each (region == box, so no margin search), and skip anything
            // near a real button. One shifted sample per button says almost nothing about how
            // often game scenery beats the threshold.
            let client = PixelRect::new(0, 0, frame.width(), frame.height());
            let b = layout.reroll(&client, 0);
            let real = layout.rerolls(&client);
            let step = (client.height / 90).max(8);
            let near_real = |x: i32, y: i32| {
                real.iter().any(|r| (r.x - x).abs() < r.width as i32 && (r.y - y).abs() < r.height as i32)
            };
            let mut y = (0.25 * client.height as f32) as i32;
            while y + b.height as i32 <= client.height as i32 {
                let mut x = 0i32;
                while x + b.width as i32 <= client.width as i32 {
                    if !near_real(x, y) {
                        if let Some(region) = crop_rgba(frame, &PixelRect::new(x, y, b.width, b.height)) {
                            let s = detector.detect(&region, b.width, b.height).score;
                            if s > best_false {
                                best_false = s;
                                false_where = format!("{name} @{x},{y}");
                            }
                        }
                    }
                    x += step as i32;
                }
                y += step as i32;
            }
        }
        // Rank on the row metric, since that is the decision production makes.
        let gap = worst_row - best_false;
        println!(
            "{bw:.4} x {bh:.4} cy {centre_y:.4}  row {worst_row:.3} ({row_where})  \
             any {worst_real:.3} ({worst_where})  false {best_false:.3} ({false_where})  gap {gap:.3}",
        );
        if gap > best.0 {
            best = (gap, (bw, bh, centre_y));
        }
    }
    let (gap, (w, h, cy)) = best;
    println!("\nbest: {w:.4} x {h:.4} centre y {cy:.4} (top {:.4}), row gap {gap:.3}", cy - h / 2.0);
}

/// Candidate template rectangles on the 3440x1440 screenshot, to pick between by eye. `card 1` is
/// an unused reroll and `card 0` a spent one, at the same offsets from their card centres.
#[test]
#[ignore = "asset tool"]
fn dump_reroll_candidates() {
    let img = load(REROLL_SCREENS[4].1);
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/reroll-crops");
    std::fs::create_dir_all(&out).unwrap();
    let layout = AugmentLayout::default();
    let client = PixelRect::new(0, 0, img.width(), img.height());
    let h = client.height as f32;
    for (w_h, hh_h) in [(0.060f32, 0.030f32), (0.066, 0.038), (0.072, 0.046), (0.080, 0.054)] {
        for (i, card) in [1usize, 0].into_iter().enumerate() {
            let _ = i;
            let cx = client.width as f32 / 2.0 + layout.card_centers[card] * h;
            // Centred on the plate: measured tops cluster at 0.6889H, heights near 0.023H, so the
            // plate's middle sits a little under 0.70H.
            let cy = 0.7045 * h;
            let (bw, bh) = ((w_h * h) as u32, (hh_h * h) as u32);
            let crop =
                image::imageops::crop_imm(&img, (cx - bw as f32 / 2.0) as u32, (cy - bh as f32 / 2.0) as u32, bw, bh)
                    .to_image();
            crop.save(out.join(format!("cand_{bw}x{bh}_card{card}.png"))).unwrap();
            println!("card {card}: {bw}x{bh} at ({:.0},{:.0})", cx - bw as f32 / 2.0, cy - bh as f32 / 2.0);
        }
    }
}

/// Dumps generous crops of every reroll button so the two states can be compared by eye and the
/// template rectangle chosen from what is actually there.
#[test]
#[ignore = "asset tool"]
fn dump_reroll_crops() {
    let layout = AugmentLayout::default();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/reroll-crops");
    std::fs::create_dir_all(&out).unwrap();
    for (name, bytes) in REROLL_SCREENS {
        let img = load(bytes);
        let client = PixelRect::new(0, 0, img.width(), img.height());
        for i in 0..3 {
            let b = search_box(&layout, &client, i);
            let crop = image::imageops::crop_imm(&img, b.x as u32, b.y as u32, b.width, b.height).to_image();
            let path = out.join(format!("{}_card{i}.png", name.replace('x', "_")));
            crop.save(&path).unwrap();
        }
    }
    println!("crops in {}", out.display());
}
