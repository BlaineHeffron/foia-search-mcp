use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::config::Config;
use crate::ingest::{OcrBackendConfig, OcrFallbackPolicy};
use crate::mcp::sources::{list_sources_note, VALID_SOURCES};
use crate::runtime::configured_sources;
use crate::sources::registry::{SOURCE_NAMES, SOURCE_REGISTRY};
use crate::sources::CachePolicy;

fn test_config() -> Config {
    Config {
        data_dir: PathBuf::from("/tmp/foia-search-test"),
        nara_api_key: Some("key".to_owned()),
        nara_api_base_url: "https://catalog.archives.gov/api/v2".to_owned(),
        ocr_fallback_policy: OcrFallbackPolicy::off(),
        ocr_backend: OcrBackendConfig::default(),
    }
}

#[test]
fn source_names_match_runtime_mcp_and_config_status() {
    let config = test_config();
    let runtime_names = configured_sources(&config)
        .into_iter()
        .map(|adapter| adapter.name())
        .collect::<Vec<_>>();
    let status_names = config
        .source_status()
        .into_iter()
        .map(|source| source.name)
        .collect::<Vec<_>>();

    let source_name_set = SOURCE_NAMES.iter().copied().collect::<BTreeSet<_>>();
    let runtime_name_set = runtime_names.iter().copied().collect::<BTreeSet<_>>();
    let valid_source_set = VALID_SOURCES.iter().copied().collect::<BTreeSet<_>>();
    let status_name_set = status_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();

    assert_eq!(SOURCE_REGISTRY.len(), SOURCE_NAMES.len());
    assert_eq!(valid_source_set, source_name_set);
    assert_eq!(runtime_name_set, source_name_set);
    assert_eq!(status_name_set, source_name_set);
}

#[test]
fn runtime_source_cache_policies_match_source_status_guidance() {
    let config = test_config();

    for adapter in configured_sources(&config) {
        let name = adapter.name();
        let status = config
            .source_status()
            .into_iter()
            .find(|source| source.name == name)
            .unwrap_or_else(|| panic!("status should include runtime source {name}"));
        let note = status.note.to_ascii_lowercase();

        match adapter.cache_policy() {
            CachePolicy::DoNotPersist => {
                assert_eq!(name, "nara");
                assert!(note.contains("donotpersist"));
            }
            CachePolicy::RespectSourceHeaders => {
                assert!(note.contains("cach"));
                assert!(note.contains("source"));
            }
        }
    }
}

#[test]
fn list_sources_notes_are_registered_for_every_runtime_source() {
    let config = test_config();

    for adapter in configured_sources(&config) {
        let note = list_sources_note(adapter.name(), true)
            .unwrap_or_else(|| panic!("list_sources note should exist for {}", adapter.name()));
        let note = note.to_ascii_lowercase();

        assert!(note.contains("cach") || note.contains("donotpersist"));
        assert!(
            note.contains("rate") || note.contains("query limit") || note.contains("query-limit")
        );
    }
}
