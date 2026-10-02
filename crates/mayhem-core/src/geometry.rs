//! Screen geometry. Positions are stored *normalised* so that they survive window moves,
//! and is converted to physical pixels only at capture time.

use serde::{Deserialize, Serialize};

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Smallest rectangle containing both.
    pub fn union(&self, other: &PixelRect) -> PixelRect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.width as i32).max(other.x + other.width as i32);
        let y1 = (self.y + self.height as i32).max(other.y + other.height as i32);
        PixelRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    /// Intersection, or `None` when the rectangles do not overlap.
    pub fn intersect(&self, other: &PixelRect) -> Option<PixelRect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.width as i32).min(other.x + other.width as i32);
        let y1 = (self.y + self.height as i32).min(other.y + other.height as i32);
        (x1 > x0 && y1 > y0).then(|| PixelRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    /// Re-expresses `self` relative to `origin`'s top-left corner.
    pub fn relative_to(&self, origin: &PixelRect) -> PixelRect {
        PixelRect::new(self.x - origin.x, self.y - origin.y, self.width, self.height)
    }
}

/// A rectangle as fractions (0..1) of some parent rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl NormRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// Resolves against a parent rectangle in pixels. Rounds outward so a thin region never
    /// collapses to zero pixels.
    pub fn to_pixels(&self, parent: &PixelRect) -> PixelRect {
        let pw = parent.width as f32;
        let ph = parent.height as f32;
        let x0 = (parent.x as f32 + self.x * pw).floor();
        let y0 = (parent.y as f32 + self.y * ph).floor();
        let x1 = (parent.x as f32 + (self.x + self.width) * pw).ceil();
        let y1 = (parent.y as f32 + (self.y + self.height) * ph).ceil();
        PixelRect::new(x0 as i32, y0 as i32, (x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32)
    }

    /// Inverse of [`NormRect::to_pixels`], used to hand pixel positions to the overlay.
    pub fn from_pixels(rect: &PixelRect, parent: &PixelRect) -> NormRect {
        let pw = parent.width.max(1) as f32;
        let ph = parent.height.max(1) as f32;
        NormRect::new(
            (rect.x - parent.x) as f32 / pw,
            (rect.y - parent.y) as f32 / ph,
            rect.width as f32 / pw,
            rect.height as f32 / ph,
        )
    }

    pub fn is_valid(&self) -> bool {
        let finite = [self.x, self.y, self.width, self.height].iter().all(|v| v.is_finite());
        finite
            && self.width > 0.0
            && self.height > 0.0
            && self.x >= 0.0
            && self.y >= 0.0
            && self.x + self.width <= 1.0 + 1e-4
            && self.y + self.height <= 1.0 + 1e-4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_rect_round_trips_through_pixels() {
        let parent = PixelRect::new(100, 50, 1920, 1080);
        let r = NormRect::new(0.25, 0.5, 0.1, 0.05);
        let px = r.to_pixels(&parent);
        assert_eq!(px, PixelRect::new(580, 590, 192, 54));
        let back = NormRect::from_pixels(&px, &parent);
        assert!((back.x - r.x).abs() < 1e-3 && (back.width - r.width).abs() < 1e-3);
    }

    #[test]
    fn union_and_intersection() {
        let a = PixelRect::new(0, 0, 10, 10);
        let b = PixelRect::new(5, 5, 10, 10);
        assert_eq!(a.union(&b), PixelRect::new(0, 0, 15, 15));
        assert_eq!(a.intersect(&b), Some(PixelRect::new(5, 5, 5, 5)));
        assert_eq!(a.intersect(&PixelRect::new(20, 20, 1, 1)), None);
    }

    #[test]
    fn validity() {
        assert!(NormRect::new(0.1, 0.1, 0.5, 0.5).is_valid());
        assert!(!NormRect::new(0.8, 0.1, 0.5, 0.5).is_valid());
        assert!(!NormRect::new(0.1, 0.1, 0.0, 0.5).is_valid());
    }
}
