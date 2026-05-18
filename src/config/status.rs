use serde::Serialize;

use super::Config;
use crate::sources::registry::SOURCE_REGISTRY;

#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub enabled: bool,
    pub status: String,
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
}
