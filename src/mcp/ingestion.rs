use rmcp::ErrorData as McpError;

use crate::{
    errors::FoiaSearchError,
    mcp::direct_ingestion_policy::TrustedDirectIngestionPolicy,
    store::{NewIngestionJob, SqliteStore, StoreError, StoredDocumentMetadata, StoredIngestionJob},
};

#[derive(Debug)]
pub(crate) struct DocumentLocator {
    pub(crate) source: String,
    pub(crate) source_id: String,
}

pub(crate) fn enqueue_ingestion_job(
    store: &mut SqliteStore,
    job_operation: &str,
    action_operation: &str,
    document_id: &str,
    force: bool,
) -> Result<StoredIngestionJob, McpError> {
    let locator = parse_document_locator(document_id)?;
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: format!("{job_operation}:{document_id}"),
            operation: job_operation.to_owned(),
            source: locator.source,
            source_id: Some(locator.source_id),
            target_url: None,
            next_action: queued_next_action(action_operation, force),
        })
        .map_err(store_error_to_mcp)
}

#[cfg(test)]
pub(crate) fn enqueue_ingestion_job_with_policy(
    store: &mut SqliteStore,
    job_operation: &str,
    action_operation: &str,
    document_id: &str,
    force: bool,
    trusted_policy: &TrustedDirectIngestionPolicy,
) -> Result<StoredIngestionJob, McpError> {
    let locator = parse_document_locator_with_policy(document_id, trusted_policy)?;
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: format!("{job_operation}:{document_id}"),
            operation: job_operation.to_owned(),
            source: locator.source,
            source_id: Some(locator.source_id),
            target_url: None,
            next_action: queued_next_action(action_operation, force),
        })
        .map_err(store_error_to_mcp)
}

pub(crate) fn enqueue_refresh_job(
    store: &mut SqliteStore,
    document_id: &str,
    force: bool,
) -> Result<StoredIngestionJob, McpError> {
    reject_direct_ingestion_locator(document_id, &TrustedDirectIngestionPolicy::deny_all())?;
    let document = resolve_refresh_document(store, document_id)?;
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: format!("refresh:{}", document.public_id),
            operation: "refresh".to_owned(),
            source: document.source,
            source_id: Some(document.source_id),
            target_url: None,
            next_action: queued_next_action("refresh", force),
        })
        .map_err(store_error_to_mcp)
}

#[cfg(test)]
pub(crate) fn enqueue_refresh_job_with_policy(
    store: &mut SqliteStore,
    document_id: &str,
    force: bool,
    _trusted_policy: &TrustedDirectIngestionPolicy,
) -> Result<StoredIngestionJob, McpError> {
    reject_direct_ingestion_locator(document_id, &TrustedDirectIngestionPolicy::deny_all())?;
    let document = resolve_refresh_document(store, document_id)?;
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: format!("refresh:{}", document.public_id),
            operation: "refresh".to_owned(),
            source: document.source,
            source_id: Some(document.source_id),
            target_url: None,
            next_action: queued_next_action("refresh", force),
        })
        .map_err(store_error_to_mcp)
}

pub(crate) fn parse_document_locator(document_id: &str) -> Result<DocumentLocator, McpError> {
    parse_document_locator_with_policy(document_id, &TrustedDirectIngestionPolicy::deny_all())
}

pub(crate) fn parse_document_locator_with_policy(
    document_id: &str,
    trusted_policy: &TrustedDirectIngestionPolicy,
) -> Result<DocumentLocator, McpError> {
    reject_direct_ingestion_locator(document_id, trusted_policy)?;

    let Some((source, source_id)) = document_id.split_once(':') else {
        return Err(FoiaSearchError::InvalidRequest(
            "document_id must use '<source>:<source_id>' format".to_owned(),
        )
        .into_mcp_error());
    };
    if source_id.trim().is_empty() {
        return Err(FoiaSearchError::InvalidRequest(
            "document_id source_id must not be empty".to_owned(),
        )
        .into_mcp_error());
    }
    super::support::validate_source(source)?;
    Ok(DocumentLocator {
        source: source.to_owned(),
        source_id: source_id.to_owned(),
    })
}

fn reject_direct_ingestion_locator(
    document_id: &str,
    trusted_policy: &TrustedDirectIngestionPolicy,
) -> Result<(), McpError> {
    let candidate = document_id.trim();
    if is_direct_url_locator(candidate) {
        return Err(direct_ingestion_policy_error());
    }
    if is_source_prefixed_direct_url_allowed(candidate, trusted_policy) {
        return Ok(());
    }
    if is_local_file_locator(candidate) || is_source_prefixed_direct_url_locator(candidate) {
        return Err(direct_ingestion_policy_error());
    }
    Ok(())
}

fn resolve_refresh_document(
    store: &SqliteStore,
    document_id: &str,
) -> Result<StoredDocumentMetadata, McpError> {
    store
        .get_document_metadata(document_id)
        .map_err(|error| match error {
            StoreError::MissingDocument(_) => FoiaSearchError::InvalidRequest(format!(
                "refresh_document requires an already-ingested local document_id or document_key; \
             no local document was found for {document_id:?}. Use search_source/get_source_record \
             followed by ingest_document, then refresh_document after ingestion succeeds."
            ))
            .into_mcp_error(),
            other => store_error_to_mcp(other),
        })
}

fn is_direct_url_locator(candidate: &str) -> bool {
    let lower = candidate.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file:")
}

fn is_local_file_locator(candidate: &str) -> bool {
    is_local_file_fragment(candidate)
        || candidate
            .split_once(':')
            .is_some_and(|(_source, source_id)| {
                let source_id = source_id.trim();
                is_local_file_fragment(source_id)
            })
}

fn is_source_prefixed_direct_url_locator(candidate: &str) -> bool {
    candidate
        .split_once(':')
        .is_some_and(|(_source, source_id)| {
            let source_id = source_id.trim();
            is_direct_url_locator(source_id)
        })
}

fn is_source_prefixed_direct_url_allowed(
    candidate: &str,
    trusted_policy: &TrustedDirectIngestionPolicy,
) -> bool {
    let Some((source, source_id)) = candidate.split_once(':') else {
        return false;
    };
    if !is_direct_url_locator(source_id.trim()) {
        return false;
    }
    trusted_policy.allows_source_id_url(source, source_id)
}

fn is_local_file_fragment(candidate: &str) -> bool {
    candidate.starts_with('/')
        || candidate.starts_with('\\')
        || candidate == "."
        || candidate.starts_with("./")
        || candidate.starts_with(".\\")
        || has_parent_path_segment(candidate)
        || is_windows_path_locator(candidate)
}

fn has_parent_path_segment(candidate: &str) -> bool {
    candidate.split(['/', '\\']).any(|segment| segment == "..")
}

fn is_windows_path_locator(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn direct_ingestion_policy_error() -> McpError {
    FoiaSearchError::InvalidRequest(
        "direct URL and local-file ingestion are disabled by default for MCP callers; use search_source or get_source_record to obtain a source-prefixed document_id such as '<source>:<source_id>', then call ingest_document or refresh_document with that ID. Enabling direct ingestion requires reviewed URL allowlists, redirect validation, size/type limits, and local path confinement."
            .to_owned(),
    )
    .into_mcp_error()
}

pub(crate) fn queued_next_action(operation: &str, force: bool) -> String {
    format!(
        "Queued for {operation}; the background worker will download assets, extract text, and index pages/chunks; force={force}."
    )
}

fn store_error_to_mcp(error: StoreError) -> McpError {
    McpError::internal_error(error.to_string(), None)
}
