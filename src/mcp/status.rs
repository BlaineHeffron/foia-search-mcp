use std::sync::Arc;

use serde::Serialize;

use crate::{
    config::{Config, OcrStatus, SourceStatus as ConfigSourceStatus},
    sources::{SourceAdapter, SourceStatus},
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ListSourcesStatus {
    pub ocr: OcrStatus,
    pub sources: Vec<ConfigSourceStatus>,
}

pub(crate) fn list_sources_status(
    config: &Config,
    adapters: &[Arc<dyn SourceAdapter>],
) -> ListSourcesStatus {
    let mut sources = config.source_status();
    for adapter in adapters {
        if let Some(status) = sources
            .iter_mut()
            .find(|status| status.name == adapter.name())
        {
            status.enabled = adapter.status() == SourceStatus::Enabled;
            if status.enabled {
                status.status = "enabled".to_owned();
            }
            status.note = crate::mcp::sources::list_sources_note(adapter.name(), status.enabled)
                .unwrap_or_else(|| status.note.clone());
        }
    }

    ListSourcesStatus {
        ocr: config.ocr_status(),
        sources,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::{
        config::Config,
        ingest::{OcrBackendConfig, OcrFallbackPolicy},
        sources::SourceAdapter,
    };

    use super::*;

    fn test_config() -> Config {
        Config {
            data_dir: PathBuf::from("/tmp/foia-search-test"),
            nara_api_key: None,
            nara_api_base_url: "https://catalog.archives.gov/api/v2".to_owned(),
            ocr_fallback_policy: OcrFallbackPolicy::off(),
            ocr_backend: OcrBackendConfig::default(),
        }
    }

    #[test]
    fn list_sources_output_reports_default_ocr_off_and_no_backend() {
        let adapters: Vec<Arc<dyn SourceAdapter>> = Vec::new();
        let output = list_sources_status(&test_config(), &adapters);

        assert_eq!(output.ocr.fallback_policy, "off");
        assert_eq!(output.ocr.backend, "none");
        assert!(!output.ocr.enabled);
        assert!(output.ocr.note.contains("disabled"));
        assert!(!output.sources.is_empty());
    }

    #[test]
    fn list_sources_output_reports_enabled_ocr_policy_and_backend() {
        let mut config = test_config();
        config.ocr_fallback_policy = OcrFallbackPolicy::on_quality_warning();
        config.ocr_backend =
            OcrBackendConfig::from_env_values(Some("ocrmypdf"), Some("ocrmypdf"), None, None);
        let adapters: Vec<Arc<dyn SourceAdapter>> = Vec::new();
        let output = list_sources_status(&config, &adapters);

        assert_eq!(output.ocr.fallback_policy, "on_quality_warning");
        assert_eq!(output.ocr.backend, "ocrmypdf");
        assert!(output.ocr.enabled);
        assert!(output.ocr.note.contains("ocrmypdf"));
    }

    #[test]
    fn list_sources_output_serializes_as_object_with_ocr_and_sources() {
        let adapters: Vec<Arc<dyn SourceAdapter>> = Vec::new();
        let output = list_sources_status(&test_config(), &adapters);
        let value = serde_json::to_value(output).expect("list_sources output should serialize");
        let object = value
            .as_object()
            .expect("list_sources output should be a top-level object");

        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            ["ocr", "sources"]
        );
        assert!(
            object
                .get("ocr")
                .and_then(serde_json::Value::as_object)
                .is_some(),
            "list_sources.ocr should be an object"
        );
        assert!(
            object
                .get("sources")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|sources| !sources.is_empty()),
            "list_sources.sources should be a non-empty array"
        );
    }

    #[test]
    fn list_sources_entries_remain_compact_source_status_objects() {
        let adapters: Vec<Arc<dyn SourceAdapter>> = Vec::new();
        let output = list_sources_status(&test_config(), &adapters);
        let value = serde_json::to_value(output).expect("list_sources output should serialize");
        let first_source = value
            .get("sources")
            .and_then(serde_json::Value::as_array)
            .and_then(|sources| sources.first())
            .and_then(serde_json::Value::as_object)
            .expect("list_sources.sources should contain source objects");

        assert_eq!(
            first_source.keys().map(String::as_str).collect::<Vec<_>>(),
            ["enabled", "name", "note", "status"]
        );
    }
}
