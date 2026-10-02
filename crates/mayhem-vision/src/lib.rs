//! Screen reading for the offered augments, which no API exposes: find the cards by their
//! frames ([`frames`]), OCR the three titles with the bundled PaddleOCR model ([`paddle`]), and
//! match them against the closed vocabulary of the Mayhem augment pool ([`matcher`], [`offer`]).
//! Everything needed ships inside the app.
//!
//! The crate reads pixels from our own desktop and nothing else: no handle to any Riot process,
//! no memory access, no injection. Platform code lives behind `cfg(windows)`
//! and behind the [`capture::ScreenCapture`] and [`ocr::OcrEngine`] traits, so all the logic here
//! is tested on any OS with synthetic images.

pub mod anvil;
pub mod button;
pub mod capture;
pub mod frames;
pub mod imageops;
pub mod matcher;
pub mod ocr;
pub mod offer;
pub mod paddle;
pub mod reroll;

#[cfg(windows)]
pub mod windows;

pub use image::{GrayImage, RgbaImage};

#[derive(Debug, thiserror::Error)]
pub enum VisionError {
    #[error("the game window was not found")]
    NoGameWindow,
    #[error("the League client window was not found")]
    NoClientWindow,
    #[error("the requested region is outside the game window")]
    RegionOutOfBounds,
    #[error("screen capture failed: {0}")]
    Capture(String),
    #[error("OCR is unavailable: {0}")]
    OcrUnavailable(String),
    #[error("OCR failed: {0}")]
    Ocr(String),
    #[error("image decoding failed: {0}")]
    Image(#[from] image::ImageError),
}
