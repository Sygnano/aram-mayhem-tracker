//! Screen capture abstraction.

use image::RgbaImage;
use mayhem_core::geometry::PixelRect;

use crate::imageops::crop_rgba;
use crate::VisionError;

pub trait ScreenCapture {
    /// The game window's client area in physical screen pixels.
    fn game_client_rect(&mut self) -> Result<PixelRect, VisionError>;
    /// Captures `rect` (physical screen pixels).
    fn capture(&mut self, rect: &PixelRect) -> Result<RgbaImage, VisionError>;
}

/// Serves a still image as if it were the game window. Used to calibrate from a screenshot and to
/// run the pipeline against recorded frames.
#[derive(Debug, Clone)]
pub struct StillCapture {
    pub frame: RgbaImage,
}

impl StillCapture {
    pub fn from_png(bytes: &[u8]) -> Result<Self, VisionError> {
        Ok(Self { frame: image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.to_rgba8() })
    }
}

impl ScreenCapture for StillCapture {
    fn game_client_rect(&mut self) -> Result<PixelRect, VisionError> {
        Ok(PixelRect::new(0, 0, self.frame.width(), self.frame.height()))
    }

    fn capture(&mut self, rect: &PixelRect) -> Result<RgbaImage, VisionError> {
        crop_rgba(&self.frame, rect).ok_or(VisionError::RegionOutOfBounds)
    }
}

/// Captures the bounding box of several regions in one grab and slices it up, which is cheaper
/// than one screen read per region.
pub fn capture_regions(capture: &mut dyn ScreenCapture, regions: &[PixelRect]) -> Result<Vec<RgbaImage>, VisionError> {
    let Some(bounds) = regions.iter().copied().reduce(|a, b| a.union(&b)) else {
        return Ok(Vec::new());
    };
    let frame = capture.capture(&bounds)?;
    regions.iter().map(|r| crop_rgba(&frame, &r.relative_to(&bounds)).ok_or(VisionError::RegionOutOfBounds)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn capture_regions_slices_one_grab() {
        let frame = RgbaImage::from_fn(100, 50, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let mut cap = StillCapture { frame };
        let parts = capture_regions(&mut cap, &[PixelRect::new(10, 5, 4, 4), PixelRect::new(60, 30, 2, 2)]).unwrap();
        assert_eq!(parts[0].get_pixel(0, 0).0, [10, 5, 0, 255]);
        assert_eq!(parts[1].get_pixel(1, 1).0, [61, 31, 0, 255]);
        assert!(cap.capture(&PixelRect::new(99, 0, 5, 5)).is_err());
    }
}
