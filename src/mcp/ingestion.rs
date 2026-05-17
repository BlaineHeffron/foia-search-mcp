use rmcp::ErrorData as McpError;

use crate::{
    errors::FoiaSearchError,
    store::{NewIngestionJob, SqliteStore, StoreError, StoredIngestionJob},
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

pub(crate) fn parse_document_locator(document_id: &str) -> Result<DocumentLocator, McpError> {
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
    super::tools::validate_source(source)?;
    Ok(DocumentLocator {
        source: source.to_owned(),
        source_id: source_id.to_owned(),
    })
}

pub(crate) fn queued_next_action(operation: &str, force: bool) -> String {
    format!(
        "Queued for {operation}; the background worker will download assets, extract text, and index pages/chunks; force={force}."
    )
}

fn store_error_to_mcp(error: StoreError) -> McpError {
    McpError::internal_error(error.to_string(), None)
}
