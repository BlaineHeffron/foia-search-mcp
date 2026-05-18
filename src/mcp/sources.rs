use rmcp::ErrorData as McpError;

use crate::errors::FoiaSearchError;
use crate::sources::registry::{source_registry_entry, SOURCE_NAMES};

pub(crate) const VALID_SOURCES: &[&str] = SOURCE_NAMES;

pub(crate) fn validate_source_name(source: &str) -> Result<(), McpError> {
    if VALID_SOURCES.contains(&source) {
        Ok(())
    } else {
        Err(FoiaSearchError::InvalidRequest(format!(
            "invalid source '{}'; expected one of: {}",
            source,
            VALID_SOURCES.join(", ")
        ))
        .into_mcp_error())
    }
}

pub(crate) fn list_sources_note(adapter_name: &str, enabled: bool) -> Option<String> {
    match (adapter_name, enabled) {
        ("nara", true) => Some(
            "NARA Catalog adapter is wired for API-key HTTP search and record fetch; API responses remain DoNotPersist by policy, with no broad scraping/caching and documented query-limit awareness."
                .to_owned(),
        ),
        (name, _) => source_registry_entry(name).map(|entry| entry.list_note.to_owned()),
    }
}
