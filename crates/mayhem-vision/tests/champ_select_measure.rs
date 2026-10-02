//! Measurement tool: finds the edges of the champ-select furniture in the real screenshots, so the
//! fitted layout is built from numbers rather than from eyeballing.
//!
//! `cargo test --release -p mayhem-vision --test champ_select_measure -- --ignored --nocapture`

use image::RgbaImage;

const MID: &[u8] = include_bytes!("fixtures/champ-select/mayhem_1920x1080_mid.png");
const FULL: &[u8] = include_bytes!("fixtures/champ-select/mayhem_1920x1080_full.png");

fn load(png: &[u8]) -> RgbaImage {
    image::load_from_memory_with_format(png, image::ImageFormat::Png).unwrap().to_rgba8()
}

fn luma(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let p = img.get_pixel(x, y).0;
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// Mean luminance of a column within a band of rows — the signal the card and strip borders show up
/// in, averaged so a single stray pixel cannot create an edge.
fn column_profile(img: &RgbaImage, y0: u32, y1: u32, x0: u32, x1: u32) -> Vec<(u32, f32)> {
    (x0..x1)
        .map(|x| {
            let sum: f32 = (y0..y1).map(|y| luma(img, x, y)).sum();
            (x, sum / (y1 - y0) as f32)
        })
        .collect()
}

fn row_profile(img: &RgbaImage, x0: u32, x1: u32, y0: u32, y1: u32) -> Vec<(u32, f32)> {
    (y0..y1)
        .map(|y| {
            let sum: f32 = (x0..x1).map(|x| luma(img, x, y)).sum();
            (y, sum / (x1 - x0) as f32)
        })
        .collect()
}

/// Positions where the profile rises through `level` going right, and where it falls back below.
fn crossings(profile: &[(u32, f32)], level: f32) -> Vec<(u32, bool)> {
    let mut out = Vec::new();
    for w in profile.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a.1 < level && b.1 >= level {
            out.push((b.0, true));
        } else if a.1 >= level && b.1 < level {
            out.push((b.0, false));
        }
    }
    out
}

fn summarise(label: &str, profile: &[(u32, f32)], level: f32) {
    let max = profile.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    let min = profile.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    println!("  {label}: min {min:.0} max {max:.0}, crossings at {level:.0}:");
    let c = crossings(profile, level);
    let edges: Vec<String> =
        c.iter().map(|(at, rising)| format!("{}{}", if *rising { "+" } else { "-" }, at)).collect();
    println!("    {}", edges.join(" "));
}

#[test]
#[ignore = "measurement tool; run explicitly with --ignored"]
fn measure_the_champ_select_furniture() {
    let mid = load(MID);
    let full = load(FULL);
    println!("mid {}x{}  full {}x{}", mid.width(), mid.height(), full.width(), full.height());

    // The two pick cards: bright rounded borders on a dark background. Sampled across the card body,
    // well below the art's top and above the name plate.
    println!("\nPICK CARDS (mid, 2 cards) — vertical edges across y=430..470");
    summarise("x", &column_profile(&mid, 430, 470, 560, 1400), 120.0);
    println!("\nPICK CARDS (mid) — horizontal edges down x=790..800 (inside the left card)");
    summarise("y", &row_profile(&mid, 790, 800, 200, 800), 120.0);

    // The available-champions strip: square portraits with dark gaps between them.
    println!("\nSTRIP (full, 7 filled) — vertical edges across y=30..75");
    summarise("x", &column_profile(&full, 30, 75, 500, 1420), 60.0);
    println!("\nSTRIP (full) — horizontal edges down x=560..580");
    summarise("y", &row_profile(&full, 560, 580, 0, 120), 60.0);

    // The ally column: circular portraits. Measured on the 'full' shot where all five are present.
    println!("\nALLY COLUMN (full) — vertical edges across y=190..215 (row 1)");
    summarise("x", &column_profile(&full, 190, 215, 0, 260), 70.0);
    println!("\nALLY COLUMN (full) — horizontal edges down x=125..135");
    summarise("y", &row_profile(&full, 125, 135, 120, 760), 70.0);
}

/// Draws the fitted layout onto the real screenshots so placement can be judged by eye — the same
/// loop that found the earlier problems. Output: `target/champ-select-fitted/`.
#[test]
#[ignore = "measurement tool; run explicitly with --ignored"]
fn draw_the_fitted_layout_over_the_real_screenshots() {
    use image::Rgba;
    use mayhem_core::champselect::ChampSelectLayout;
    use mayhem_core::geometry::PixelRect;

    fn outline(img: &mut RgbaImage, r: &PixelRect, colour: Rgba<u8>) {
        let (w, h) = (img.width() as i32, img.height() as i32);
        for t in 0..3 {
            for x in r.x - t..r.x + r.width as i32 + t {
                for y in [r.y - t, r.y + r.height as i32 + t] {
                    if (0..w).contains(&x) && (0..h).contains(&y) {
                        img.put_pixel(x as u32, y as u32, colour);
                    }
                }
            }
            for y in r.y - t..r.y + r.height as i32 + t {
                for x in [r.x - t, r.x + r.width as i32 + t] {
                    if (0..w).contains(&x) && (0..h).contains(&y) {
                        img.put_pixel(x as u32, y as u32, colour);
                    }
                }
            }
        }
    }

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/champ-select-fitted");
    std::fs::create_dir_all(&out).unwrap();
    let layout = ChampSelectLayout::default();

    for (label, png, cards) in [("mid", MID, 2usize), ("full", FULL, 2), ("full-as-three", FULL, 3)] {
        let mut canvas = load(png);
        let client = PixelRect::new(0, 0, canvas.width(), canvas.height());
        // Green: the pick cards. Blue: the available-champions strip. Orange: the ally column.
        for r in layout.pick_cards(&client, cards) {
            outline(&mut canvas, &r, Rgba([60, 255, 90, 255]));
        }
        for r in layout.strip_slots(&client, 10) {
            outline(&mut canvas, &r, Rgba([90, 180, 255, 255]));
        }
        for r in layout.ally_portraits(&client) {
            outline(&mut canvas, &r, Rgba([255, 170, 40, 255]));
        }
        let path = out.join(format!("{label}.png"));
        canvas.save(&path).unwrap();
        println!("drawn -> {}", path.display());
    }
}

const THREE: &[u8] = include_bytes!("fixtures/champ-select/mayhem_1920x1080_three_cards.png");

/// Draws the statistics blocks over the real captures, so their placement can be judged by eye.
/// Output: `target/champ-select-blocks/`.
#[test]
#[ignore = "measurement tool; run explicitly with --ignored"]
fn draw_the_statistics_blocks_over_the_real_screenshots() {
    use image::Rgba;
    use mayhem_core::champselect::ChampSelectLayout;
    use mayhem_core::geometry::PixelRect;

    fn fill(img: &mut RgbaImage, r: &PixelRect, colour: Rgba<u8>) {
        let (w, h) = (img.width() as i32, img.height() as i32);
        for y in r.y..r.y + r.height as i32 {
            for x in r.x..r.x + r.width as i32 {
                if (0..w).contains(&x) && (0..h).contains(&y) {
                    let edge =
                        x < r.x + 3 || x >= r.x + r.width as i32 - 3 || y < r.y + 3 || y >= r.y + r.height as i32 - 3;
                    let p = img.get_pixel(x as u32, y as u32).0;
                    let mix = |a: u8, b: u8| if edge { b } else { ((a as u16 * 3 + b as u16) / 4) as u8 };
                    img.put_pixel(
                        x as u32,
                        y as u32,
                        Rgba([mix(p[0], colour.0[0]), mix(p[1], colour.0[1]), mix(p[2], colour.0[2]), 255]),
                    );
                }
            }
        }
    }

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/champ-select-blocks");
    std::fs::create_dir_all(&out).unwrap();
    let layout = ChampSelectLayout::default();

    // Counts come from the LCU, exactly as the app gets them: the offer and the bench are state the
    // client reports, not something to measure off a picture. The numbers here are what each capture
    // showed.
    for (label, png, cards, bench) in [("three_cards", THREE, 3usize, 0usize), ("mid", MID, 2, 4), ("full", FULL, 0, 7)]
    {
        let mut canvas = load(png);
        let client = PixelRect::new(0, 0, canvas.width(), canvas.height());

        // Green: the pick cards. Blue: the bench, which is also the swap button. Orange: allies.
        for r in layout.pick_card_blocks(&client, cards) {
            fill(&mut canvas, &r, Rgba([60, 255, 90, 255]));
        }
        for r in layout.strip_blocks(&client, bench) {
            fill(&mut canvas, &r, Rgba([90, 180, 255, 255]));
        }
        for r in layout.ally_blocks(&client) {
            fill(&mut canvas, &r, Rgba([255, 170, 40, 255]));
        }
        let path = out.join(format!("{label}.png"));
        canvas.save(&path).unwrap();
        println!("{label}: {cards} cards, {bench} on the bench -> {}", path.display());
    }
}
