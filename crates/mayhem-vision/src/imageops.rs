//! Small image utilities: greyscale conversion, resampling and normalised cross-correlation.

use image::imageops::{self, FilterType};
use image::{GrayImage, Luma, RgbaImage};
use mayhem_core::geometry::PixelRect;

/// Rec. 601 luma.
pub fn to_gray(img: &RgbaImage) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        Luma([l.round().clamp(0.0, 255.0) as u8])
    })
}

/// Crops `rect` (relative to the image origin). Returns `None` if it does not fit.
pub fn crop_rgba(img: &RgbaImage, rect: &PixelRect) -> Option<RgbaImage> {
    let fits = rect.x >= 0
        && rect.y >= 0
        && !rect.is_empty()
        && rect.x as u32 + rect.width <= img.width()
        && rect.y as u32 + rect.height <= img.height();
    fits.then(|| imageops::crop_imm(img, rect.x as u32, rect.y as u32, rect.width, rect.height).to_image())
}

pub fn resize_gray(img: &GrayImage, width: u32, height: u32) -> GrayImage {
    if img.width() == width && img.height() == height {
        return img.clone();
    }
    imageops::resize(img, width.max(1), height.max(1), FilterType::Triangle)
}

/// NCC of `template` against the window of `img` at `(ox, oy)` with the template's size.
fn ncc_window(img: &GrayImage, ox: u32, oy: u32, template: &GrayImage) -> f32 {
    let (tw, th) = template.dimensions();
    let n = (tw * th) as f64;
    let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0f64, 0f64, 0f64, 0f64, 0f64);
    for y in 0..th {
        for x in 0..tw {
            let a = img.get_pixel(ox + x, oy + y).0[0] as f64;
            let b = template.get_pixel(x, y).0[0] as f64;
            sa += a;
            sb += b;
            saa += a * a;
            sbb += b * b;
            sab += a * b;
        }
    }
    let cov = sab - sa * sb / n;
    let va = saa - sa * sa / n;
    let vb = sbb - sb * sb / n;
    if va <= 1e-6 || vb <= 1e-6 {
        return 0.0;
    }
    (cov / (va * vb).sqrt()) as f32
}

/// Where a template correlated best, in `img`'s pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NccPeak {
    pub score: f32,
    /// Top-left corner of the template's placement.
    pub x: u32,
    pub y: u32,
}

/// The single best placement of `template` inside `img`, or `None` if it does not fit. Searching
/// every placement, rather than scoring one, absorbs a pixel or two of layout error.
pub fn locate_ncc(img: &GrayImage, template: &GrayImage) -> Option<NccPeak> {
    let (iw, ih) = img.dimensions();
    let (tw, th) = template.dimensions();
    if tw > iw || th > ih || tw == 0 || th == 0 {
        return None;
    }
    let mut best: Option<NccPeak> = None;
    for y in 0..=ih - th {
        for x in 0..=iw - tw {
            let score = ncc_window(img, x, y, template);
            if best.is_none_or(|b| score > b.score) {
                best = Some(NccPeak { score, x, y });
            }
        }
    }
    best
}

#[cfg(test)]
pub(crate) mod testing {
    use image::{GrayImage, Luma};

    /// Deterministic textured background (so NCC has structure to work with).
    pub fn texture(w: u32, h: u32, seed: u32) -> GrayImage {
        GrayImage::from_fn(w, h, |x, y| {
            let v = (x.wrapping_mul(73) ^ y.wrapping_mul(151) ^ seed.wrapping_mul(2654435761)) % 97;
            Luma([40 + v as u8])
        })
    }

    /// Draws a bright plus sign (a simple icon stand-in) centred at (cx, cy).
    pub fn draw_plus(img: &mut GrayImage, cx: u32, cy: u32, arm: u32) {
        for d in 0..=arm * 2 {
            for t in 0..2 {
                img.put_pixel(cx - arm + d, cy - 1 + t, Luma([250]));
                img.put_pixel(cx - 1 + t, cy - arm + d, Luma([250]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn ncc_is_contrast_invariant() {
        let a = texture(12, 12, 1);
        let dim = GrayImage::from_fn(12, 12, |x, y| Luma([a.get_pixel(x, y).0[0] / 2 + 10]));
        let ncc = |b: &GrayImage| ncc_window(&a, 0, 0, b);
        assert!(ncc(&dim) > 0.99);
        assert!(ncc(&texture(12, 12, 7)) < 0.5);
        assert_eq!(ncc(&GrayImage::from_pixel(12, 12, Luma([9]))), 0.0);
    }

    #[test]
    fn the_best_placement_is_where_the_template_was_cut_from() {
        let mut img = texture(20, 20, 3);
        draw_plus(&mut img, 11, 9, 4);
        let template = imageops::crop_imm(&img, 5, 3, 12, 12).to_image();
        let peak = locate_ncc(&img, &template).unwrap();
        assert!(peak.score > 0.999);
        assert_eq!((peak.x, peak.y), (5, 3));
        assert!(locate_ncc(&template, &img).is_none(), "a template larger than the image does not fit");
    }

    #[test]
    fn crop_bounds_are_checked() {
        let img = RgbaImage::new(10, 10);
        assert!(crop_rgba(&img, &PixelRect::new(2, 2, 8, 8)).is_some());
        assert!(crop_rgba(&img, &PixelRect::new(3, 2, 8, 8)).is_none());
        assert!(crop_rgba(&img, &PixelRect::new(-1, 0, 2, 2)).is_none());
    }
}
