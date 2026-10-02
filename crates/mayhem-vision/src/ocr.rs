//! OCR engine abstraction. The real engine is the bundled PaddleOCR model ([`crate::paddle`]);
//! tests can also use a scripted fake.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::VisionError;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrText {
    pub lines: Vec<String>,
}

impl OcrText {
    /// All lines on one line: a card title may wrap, and the matcher ignores spacing anyway.
    pub fn joined(&self) -> String {
        self.lines.join(" ")
    }
}

pub trait OcrEngine {
    fn describe(&self) -> String;
    fn recognize(&self, image: &RgbaImage) -> Result<OcrText, VisionError>;

    /// Several single-line crops at once. Engines that can batch override this.
    fn recognize_all(&self, images: &[RgbaImage]) -> Result<Vec<OcrText>, VisionError> {
        images.iter().map(|i| self.recognize(i)).collect()
    }
}

/// Replays canned results, keyed by call order. For tests and for replaying recorded sessions.
#[derive(Debug, Default)]
pub struct ScriptedOcr {
    pub results: std::cell::RefCell<std::collections::VecDeque<String>>,
}

impl ScriptedOcr {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(results: I) -> Self {
        Self { results: std::cell::RefCell::new(results.into_iter().map(Into::into).collect()) }
    }
}

impl OcrEngine for ScriptedOcr {
    fn describe(&self) -> String {
        "scripted".into()
    }

    fn recognize(&self, _image: &RgbaImage) -> Result<OcrText, VisionError> {
        let next = self.results.borrow_mut().pop_front().unwrap_or_default();
        Ok(OcrText { lines: next.lines().map(str::to_owned).collect() })
    }
}
