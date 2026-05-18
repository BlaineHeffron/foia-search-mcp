use std::path::PathBuf;

use crate::ingest::{OcrBackendConfig, OcrFallbackPolicy};

mod status;
pub use status::SourceStatus;

const DEFAULT_DATA_DIR_NAME: &str = ".foia-search";

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub nara_api_key: Option<String>,
    pub nara_api_base_url: String,
    pub ocr_fallback_policy: OcrFallbackPolicy,
    pub ocr_backend: OcrBackendConfig,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = std::env::var("FOIA_SEARCH_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir_or_current().join(DEFAULT_DATA_DIR_NAME));
        let nara_api_key = std::env::var("FOIA_SEARCH_NARA_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let nara_api_base_url = std::env::var("FOIA_SEARCH_NARA_API_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "https://catalog.archives.gov/api/v2".to_owned());
        let ocr_fallback_policy = OcrFallbackPolicy::from_env_value(
            std::env::var("FOIA_SEARCH_OCR_FALLBACK").ok().as_deref(),
        );
        let ocr_backend = OcrBackendConfig::from_env_values(
            std::env::var("FOIA_SEARCH_OCR_BACKEND").ok().as_deref(),
            std::env::var("FOIA_SEARCH_OCRMYPDF_BIN").ok().as_deref(),
            std::env::var("FOIA_SEARCH_OCR_TIMEOUT_SECONDS")
                .ok()
                .as_deref(),
            std::env::var("FOIA_SEARCH_OCR_MAX_STDERR_BYTES")
                .ok()
                .as_deref(),
        );

        Self {
            data_dir,
            nara_api_key,
            nara_api_base_url,
            ocr_fallback_policy,
            ocr_backend,
        }
    }
}

fn home_dir_or_current() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::OcrBackend;

    #[test]
    fn ocr_backend_defaults_disabled_without_env_values() {
        let config = OcrBackendConfig::from_env_values(None, None, None, None);

        assert_eq!(config.backend, OcrBackend::None);
        assert!(!config.backend.is_enabled());
    }
}
