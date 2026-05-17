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
                status: "available".to_string(),
                note: "GovInfo live API adapter is available for Search Service queries and package/granule summary fetches; prefer PDF/XML/MODS links.".to_string(),
            },
            SourceStatus {
                name: "pursue".to_string(),
                enabled: false,
                status: "available".to_string(),
                note: "PURSUE/war.gov UAP release adapter is available for tranche/record leads and official release assets; PDFs are ingest-preferred while images/videos remain metadata assets.".to_string(),
            },
            SourceStatus {
                name: "doj_epstein".to_string(),
                enabled: false,
                status: "available".to_string(),
                note: "DOJ Epstein Library adapter is available for official DOJ disclosure leads; preserve sensitivity/privacy warnings and prefer PDF ingestion while images/audio/video remain metadata assets.".to_string(),
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
    fn source_status_reports_govinfo_live_adapter_note() {
        let config = Config {
            data_dir: PathBuf::from("/tmp/foia-search-test"),
            nara_api_key: None,
            nara_api_base_url: "https://catalog.archives.gov/api/v2".to_string(),
            ocr_fallback_policy: OcrFallbackPolicy::off(),
            ocr_backend: OcrBackendConfig::default(),
        };

        let govinfo = config
            .source_status()
            .into_iter()
            .find(|source| source.name == "govinfo")
            .expect("govinfo source should be listed");

        assert_eq!(govinfo.status, "available");
        assert!(govinfo.note.contains("live API adapter"));
        assert!(!govinfo.note.contains("manual"));
    }

    #[test]
    fn source_status_reports_pursue_available_adapter_note() {
        let config = Config {
            data_dir: PathBuf::from("/tmp/foia-search-test"),
            nara_api_key: None,
            nara_api_base_url: "https://catalog.archives.gov/api/v2".to_string(),
            ocr_fallback_policy: OcrFallbackPolicy::off(),
            ocr_backend: OcrBackendConfig::default(),
        };

        let pursue = config
            .source_status()
            .into_iter()
            .find(|source| source.name == "pursue")
            .expect("pursue source should be listed");

        assert_eq!(pursue.status, "available");
        assert!(pursue.note.contains("PURSUE/war.gov UAP release adapter"));
        assert!(pursue.note.contains("images/videos remain metadata assets"));
    }

    #[test]
    fn source_status_reports_doj_epstein_available_adapter_note() {
        let config = Config {
            data_dir: PathBuf::from("/tmp/foia-search-test"),
            nara_api_key: None,
            nara_api_base_url: "https://catalog.archives.gov/api/v2".to_string(),
            ocr_fallback_policy: OcrFallbackPolicy::off(),
            ocr_backend: OcrBackendConfig::default(),
        };

        let doj_epstein = config
            .source_status()
            .into_iter()
            .find(|source| source.name == "doj_epstein")
            .expect("doj_epstein source should be listed");

        assert_eq!(doj_epstein.status, "available");
        assert!(doj_epstein.note.contains("DOJ Epstein Library adapter"));
        assert!(doj_epstein.note.contains("sensitivity/privacy warnings"));
    }

    #[test]
    fn ocr_backend_defaults_disabled_without_env_values() {
        let config = OcrBackendConfig::from_env_values(None, None, None, None);

        assert_eq!(config.backend, OcrBackend::None);
        assert!(!config.backend.is_enabled());
    }
}
