use serde::Serialize;

use super::Config;
use crate::ingest::OcrBackend;
use crate::sources::registry::SOURCE_REGISTRY;

#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub enabled: bool,
    pub status: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrBackendAvailability {
    pub backend: String,
    pub availability: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrStatus {
    pub fallback_policy: String,
    pub selectable_modes: Vec<String>,
    pub backend: String,
    pub backend_availability: Vec<OcrBackendAvailability>,
    pub backend_binary: Option<String>,
    pub enabled: bool,
    pub note: String,
}

impl Config {
    pub fn source_status(&self) -> Vec<SourceStatus> {
        SOURCE_REGISTRY
            .iter()
            .map(|entry| {
                let (status, note) = if entry.name == "nara" && self.nara_api_key.is_some() {
                    (
                        "configured",
                        "FOIA_SEARCH_NARA_API_KEY is set; NARA Catalog adapter is available with DoNotPersist API response handling, no broad scraping/caching, and documented query-limit awareness.",
                    )
                } else {
                    (entry.status, entry.config_note)
                };
                SourceStatus {
                    name: entry.name.to_owned(),
                    enabled: false,
                    status: status.to_owned(),
                    note: note.to_owned(),
                }
            })
            .collect()
    }

    pub fn ocr_status(&self) -> OcrStatus {
        let fallback_policy = if self.ocr_fallback_policy.is_enabled() {
            "on_quality_warning"
        } else {
            "off"
        };
        let selectable_modes = vec!["off".to_owned(), "on_quality_warning".to_owned()];
        let backend = match self.ocr_backend.backend {
            OcrBackend::None => "none",
            OcrBackend::Ocrmypdf => "ocrmypdf",
            OcrBackend::Tesseract => "tesseract",
        };
        let backend_availability = vec![
            OcrBackendAvailability {
                backend: "none".to_owned(),
                availability: "disabled".to_owned(),
            },
            OcrBackendAvailability {
                backend: "ocrmypdf".to_owned(),
                availability: "supported_requires_binary".to_owned(),
            },
            OcrBackendAvailability {
                backend: "tesseract".to_owned(),
                availability: "reserved_unavailable".to_owned(),
            },
        ];
        let backend_binary = (self.ocr_backend.backend == OcrBackend::Ocrmypdf)
            .then(|| self.ocr_backend.ocrmypdf_binary.display().to_string());
        let enabled =
            self.ocr_fallback_policy.is_enabled() && self.ocr_backend.backend.is_enabled();
        let note = match (
            self.ocr_fallback_policy.is_enabled(),
            self.ocr_backend.backend,
        ) {
            (_, OcrBackend::Tesseract) => "FOIA_SEARCH_OCR_BACKEND=tesseract is recognized but unavailable: tesseract OCR extraction is not implemented in this build. Use FOIA_SEARCH_OCR_BACKEND=ocrmypdf for the current local OCR backend, or leave FOIA_SEARCH_OCR_BACKEND unset to keep OCR disabled.",
            (false, _) => "Local OCR fallback is disabled. To enable it for low-quality or failed embedded PDF text extraction, set FOIA_SEARCH_OCR_FALLBACK=on_quality_warning and FOIA_SEARCH_OCR_BACKEND=ocrmypdf; install ocrmypdf or set FOIA_SEARCH_OCRMYPDF_BIN if the binary is not on PATH.",
            (true, OcrBackend::None) => "Local OCR fallback policy is enabled, but no OCR backend is configured. Set FOIA_SEARCH_OCR_BACKEND=ocrmypdf and install ocrmypdf or set FOIA_SEARCH_OCRMYPDF_BIN.",
            (true, OcrBackend::Ocrmypdf) => "Local OCR fallback is enabled for low-quality or failed embedded PDF text extraction using ocrmypdf. If OCR jobs report a missing binary, install ocrmypdf or set FOIA_SEARCH_OCRMYPDF_BIN to the executable path.",
        };

        OcrStatus {
            fallback_policy: fallback_policy.to_owned(),
            selectable_modes,
            backend: backend.to_owned(),
            backend_availability,
            backend_binary,
            enabled,
            note: note.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::ingest::{OcrBackendConfig, OcrFallbackPolicy};
    use crate::sources::registry::SOURCE_NAMES;

    use super::*;

    fn test_config(nara_api_key: Option<&str>) -> Config {
        Config {
            data_dir: PathBuf::from("/tmp/foia-search-test"),
            nara_api_key: nara_api_key.map(str::to_owned),
            nara_api_base_url: "https://catalog.archives.gov/api/v2".to_owned(),
            ocr_fallback_policy: OcrFallbackPolicy::off(),
            ocr_backend: OcrBackendConfig::default(),
        }
    }

    fn status_for(config: &Config, name: &str) -> SourceStatus {
        config
            .source_status()
            .into_iter()
            .find(|source| source.name == name)
            .unwrap_or_else(|| panic!("source status should include {name}"))
    }

    #[test]
    fn source_status_names_follow_registry() {
        let config = test_config(None);
        let status_names = config
            .source_status()
            .into_iter()
            .map(|source| source.name)
            .collect::<Vec<_>>();

        assert_eq!(status_names, SOURCE_NAMES);
    }

    #[test]
    fn source_status_reports_nara_key_state_and_do_not_persist_policy() {
        let missing = test_config(None);
        let nara_missing = status_for(&missing, "nara");
        assert_eq!(nara_missing.status, "missing_api_key");
        assert!(nara_missing.note.contains("FOIA_SEARCH_NARA_API_KEY"));
        assert!(nara_missing.note.contains("DoNotPersist"));
        assert!(nara_missing.note.contains("query limits"));

        let configured = test_config(Some("key"));
        let nara_configured = status_for(&configured, "nara");
        assert_eq!(nara_configured.status, "configured");
        assert!(nara_configured
            .note
            .contains("FOIA_SEARCH_NARA_API_KEY is set"));
        assert!(nara_configured.note.contains("DoNotPersist"));
        assert!(nara_configured.note.contains("query-limit"));
    }

    #[test]
    fn every_source_status_note_mentions_cache_or_persistence_policy() {
        let config = test_config(None);

        for source in config.source_status() {
            let note = source.note.to_ascii_lowercase();
            assert!(
                note.contains("cach")
                    || note.contains("donotpersist")
                    || note.contains("do not persist"),
                "{} note should mention cache or persistence policy: {}",
                source.name,
                source.note
            );
        }
    }

    #[test]
    fn every_source_status_note_mentions_rate_or_query_guidance() {
        let config = test_config(None);

        for source in config.source_status() {
            let note = source.note.to_ascii_lowercase();
            assert!(
                note.contains("rate")
                    || note.contains("query limit")
                    || note.contains("query-limit"),
                "{} note should mention rate/query guidance: {}",
                source.name,
                source.note
            );
        }
    }

    #[test]
    fn ocr_status_defaults_to_off_with_no_backend_guidance() {
        let config = test_config(None);
        let status = config.ocr_status();

        assert_eq!(status.fallback_policy, "off");
        assert_eq!(status.selectable_modes, vec!["off", "on_quality_warning"]);
        assert_eq!(status.backend, "none");
        assert_eq!(status.backend_availability.len(), 3);
        assert_eq!(status.backend_availability[0].backend, "none");
        assert_eq!(status.backend_availability[0].availability, "disabled");
        assert_eq!(status.backend_availability[1].backend, "ocrmypdf");
        assert_eq!(
            status.backend_availability[1].availability,
            "supported_requires_binary"
        );
        assert_eq!(status.backend_availability[2].backend, "tesseract");
        assert_eq!(
            status.backend_availability[2].availability,
            "reserved_unavailable"
        );
        assert_eq!(status.backend_binary, None);
        assert!(!status.enabled);
        assert!(status
            .note
            .contains("FOIA_SEARCH_OCR_FALLBACK=on_quality_warning"));
        assert!(status.note.contains("FOIA_SEARCH_OCR_BACKEND=ocrmypdf"));
    }

    #[test]
    fn ocr_status_reports_configured_policy_backend_and_binary_guidance() {
        let mut config = test_config(None);
        config.ocr_fallback_policy = OcrFallbackPolicy::on_quality_warning();
        config.ocr_backend = OcrBackendConfig::from_env_values(
            Some("ocrmypdf"),
            Some("/usr/local/bin/ocrmypdf"),
            None,
            None,
        );
        let status = config.ocr_status();

        assert_eq!(status.fallback_policy, "on_quality_warning");
        assert_eq!(status.selectable_modes, vec!["off", "on_quality_warning"]);
        assert_eq!(status.backend, "ocrmypdf");
        assert_eq!(
            status.backend_binary.as_deref(),
            Some("/usr/local/bin/ocrmypdf")
        );
        assert!(status.enabled);
        assert!(status.note.contains("missing binary"));
        assert!(status.note.contains("FOIA_SEARCH_OCRMYPDF_BIN"));
    }

    #[test]
    fn ocr_status_reports_tesseract_as_unavailable_not_enabled() {
        let mut config = test_config(None);
        config.ocr_fallback_policy = OcrFallbackPolicy::on_quality_warning();
        config.ocr_backend = OcrBackendConfig::from_env_values(Some("tesseract"), None, None, None);
        let status = config.ocr_status();

        assert_eq!(status.fallback_policy, "on_quality_warning");
        assert_eq!(status.selectable_modes, vec!["off", "on_quality_warning"]);
        assert_eq!(status.backend, "tesseract");
        assert_eq!(status.backend_binary, None);
        assert!(!status.enabled);
        assert!(status.note.contains("recognized but unavailable"));
        assert!(status.note.contains("not implemented"));
        assert!(status.note.contains("FOIA_SEARCH_OCR_BACKEND=ocrmypdf"));
    }

    #[test]
    fn ocr_status_reports_tesseract_unavailable_even_when_fallback_policy_is_off() {
        let mut config = test_config(None);
        config.ocr_backend = OcrBackendConfig::from_env_values(Some("tesseract"), None, None, None);
        let status = config.ocr_status();

        assert_eq!(status.fallback_policy, "off");
        assert_eq!(status.selectable_modes, vec!["off", "on_quality_warning"]);
        assert_eq!(status.backend, "tesseract");
        assert!(!status.enabled);
        assert!(status.note.contains("recognized but unavailable"));
        assert!(status.note.contains("not implemented"));
    }
}
