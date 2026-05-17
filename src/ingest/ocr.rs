use crate::ingest::pdf::{ExtractedText, TextExtraction, TextExtractor};
use std::path::Path;

pub const OCR_FALLBACK_USED_WARNING: &str =
    "local OCR fallback was used after embedded PDF text quality warnings";
pub const OCR_FALLBACK_RESCUED_WARNING: &str =
    "local OCR fallback was used after embedded PDF text extraction failed";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OcrFallbackPolicy {
    mode: OcrFallbackMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OcrFallbackMode {
    #[default]
    Off,
    OnQualityWarning,
}

impl OcrFallbackPolicy {
    pub fn off() -> Self {
        Self {
            mode: OcrFallbackMode::Off,
        }
    }

    pub fn on_quality_warning() -> Self {
        Self {
            mode: OcrFallbackMode::OnQualityWarning,
        }
    }

    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("on_quality_warning") => Self::on_quality_warning(),
            _ => Self::off(),
        }
    }

    pub fn is_enabled(self) -> bool {
        self.mode == OcrFallbackMode::OnQualityWarning
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopOcrExtractor;

impl TextExtractor for NoopOcrExtractor {
    fn extract_pages(&self, _path: &Path) -> Result<ExtractedText, TextExtraction> {
        Err(TextExtraction::UnavailableBinary {
            binary: "local-ocr-fallback-disabled".into(),
        })
    }
}
