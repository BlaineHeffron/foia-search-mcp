use crate::ingest::pdf::{ExtractedText, TextExtraction, TextExtractor};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

pub const OCR_FALLBACK_USED_WARNING: &str =
    "local OCR fallback was used after embedded PDF text quality warnings";
pub const OCR_FALLBACK_RESCUED_WARNING: &str =
    "local OCR fallback was used after embedded PDF text extraction failed";
pub const OCR_FALLBACK_INCOMPATIBLE_WARNING: &str =
    "local OCR fallback output was ignored because OCR page boundaries/page numbers did not match embedded PDF text";
const DEFAULT_OCR_TIMEOUT: Duration = Duration::from_secs(300);
const DEFAULT_MAX_STDERR_BYTES: usize = 8 * 1024;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OcrBackend {
    #[default]
    None,
    Ocrmypdf,
    Tesseract,
}

impl OcrBackend {
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("ocrmypdf") => Self::Ocrmypdf,
            Some("tesseract") => Self::Tesseract,
            _ => Self::None,
        }
    }

    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Ocrmypdf)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OcrBackendConfig {
    pub backend: OcrBackend,
    pub ocrmypdf_binary: PathBuf,
    pub timeout: Duration,
    pub max_stderr_bytes: usize,
}

impl OcrBackendConfig {
    pub fn from_env_values(
        backend: Option<&str>,
        ocrmypdf_binary: Option<&str>,
        timeout_seconds: Option<&str>,
        max_stderr_bytes: Option<&str>,
    ) -> Self {
        Self {
            backend: OcrBackend::from_env_value(backend),
            ocrmypdf_binary: ocrmypdf_binary
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("ocrmypdf")),
            timeout: parse_positive_seconds(timeout_seconds).unwrap_or(DEFAULT_OCR_TIMEOUT),
            max_stderr_bytes: parse_positive_usize(max_stderr_bytes)
                .unwrap_or(DEFAULT_MAX_STDERR_BYTES),
        }
    }
}

impl Default for OcrBackendConfig {
    fn default() -> Self {
        Self::from_env_values(None, None, None, None)
    }
}

fn parse_positive_seconds(value: Option<&str>) -> Option<Duration> {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
}

fn parse_positive_usize(value: Option<&str>) -> Option<usize> {
    value
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|bytes| *bytes > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_backend_config_defaults_to_disabled_ocrmypdf_settings() {
        let config = OcrBackendConfig::default();

        assert_eq!(config.backend, OcrBackend::None);
        assert_eq!(config.ocrmypdf_binary, PathBuf::from("ocrmypdf"));
        assert_eq!(config.timeout, Duration::from_secs(300));
        assert_eq!(config.max_stderr_bytes, 8 * 1024);
    }

    #[test]
    fn ocr_backend_config_parses_explicit_ocrmypdf_backend() {
        let config = OcrBackendConfig::from_env_values(
            Some("ocrmypdf"),
            Some("/opt/bin/ocrmypdf"),
            Some("12"),
            Some("256"),
        );

        assert_eq!(config.backend, OcrBackend::Ocrmypdf);
        assert_eq!(config.ocrmypdf_binary, PathBuf::from("/opt/bin/ocrmypdf"));
        assert_eq!(config.timeout, Duration::from_secs(12));
        assert_eq!(config.max_stderr_bytes, 256);
    }

    #[test]
    fn ocr_backend_config_parses_tesseract_as_unimplemented_backend() {
        let config = OcrBackendConfig::from_env_values(
            Some("tesseract"),
            Some("  "),
            Some("0"),
            Some("invalid"),
        );

        assert_eq!(config.backend, OcrBackend::Tesseract);
        assert_eq!(config.ocrmypdf_binary, PathBuf::from("ocrmypdf"));
        assert_eq!(config.timeout, Duration::from_secs(300));
        assert_eq!(config.max_stderr_bytes, 8 * 1024);
    }

    #[test]
    fn ocr_backend_config_ignores_unknown_values() {
        let config = OcrBackendConfig::from_env_values(Some("unknown"), None, None, None);

        assert_eq!(config.backend, OcrBackend::None);
    }
}
