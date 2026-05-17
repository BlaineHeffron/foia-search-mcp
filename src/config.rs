use std::path::PathBuf;

use crate::ingest::{OcrBackendConfig, OcrFallbackPolicy};
use serde::Serialize;

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

    pub fn source_status(&self) -> Vec<SourceStatus> {
        vec![
            SourceStatus {
                name: "cia".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "CIA Reading Room adapter is available for HTTP search and record fetch."
                    .to_string(),
            },
            SourceStatus {
                name: "nara".to_string(),
                enabled: false,
                status: if self.nara_api_key.is_some() {
                    "configured"
                } else {
                    "missing_api_key"
                }
                .to_string(),
                note: if self.nara_api_key.is_some() {
                    "FOIA_SEARCH_NARA_API_KEY is set; NARA Catalog adapter is available with non-persistent API response handling."
                } else {
                    "Set FOIA_SEARCH_NARA_API_KEY before enabling the NARA adapter."
                }
                .to_string(),
            },
            SourceStatus {
                name: "govinfo".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "GovInfo adapter is planned after CIA and NARA.".to_string(),
            },
            SourceStatus {
                name: "frus".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "FRUS adapter is planned for document-level citations.".to_string(),
            },
            SourceStatus {
                name: "dtic".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "DTIC adapter is planned and will require fragility warnings.".to_string(),
            },
            SourceStatus {
                name: "noaa".to_string(),
                enabled: false,
                status: "planned".to_string(),
                note: "NOAA repository adapter is planned.".to_string(),
            },
        ]
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub enabled: bool,
    pub status: String,
    pub note: String,
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
    fn source_status_reports_nara_key_state() {
        let config = Config {
            data_dir: PathBuf::from("/tmp/foia-search-test"),
            nara_api_key: Some("key".to_string()),
            nara_api_base_url: "https://catalog.archives.gov/api/v2".to_string(),
            ocr_fallback_policy: OcrFallbackPolicy::off(),
            ocr_backend: OcrBackendConfig::default(),
        };

        let nara = config
            .source_status()
            .into_iter()
            .find(|source| source.name == "nara");

        assert!(matches!(
            nara.as_ref().map(|source| source.status.as_str()),
            Some("configured")
        ));
    }

    #[test]
    fn ocr_backend_defaults_disabled_without_env_values() {
        let config = OcrBackendConfig::from_env_values(None, None, None, None);

        assert_eq!(config.backend, OcrBackend::None);
        assert!(!config.backend.is_enabled());
    }
}
