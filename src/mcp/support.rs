use crate::errors::FoiaSearchError;
use crate::index::SearchHit;
use crate::model::{IngestionJob, LocalDocument, LocalDocumentText, LocalPageText, LocalSearchHit};
use crate::store::{StoreError, StoredDocumentMetadata, StoredIngestionJob, StoredPageText};
use rmcp::ErrorData as McpError;
use serde_json::Value;

pub(crate) const MAX_TEXT_PAGE_RANGE: u32 = 50;

pub(crate) fn validate_source(source: &str) -> Result<(), McpError> {
    crate::mcp::sources::validate_source_name(source)
}

pub(crate) fn ingestion_job_from_stored(job: StoredIngestionJob) -> IngestionJob {
    let document_id = job
        .source_id
        .as_ref()
        .map(|source_id| format!("{}:{source_id}", job.source))
        .or(job.target_url.clone());
    let mut next_actions = Vec::new();
    if let Some(next_action) = job.next_action {
        next_actions.push(next_action);
    }
    if next_actions.is_empty() {
        next_actions.push(format!("Current stage is '{}'.", job.stage));
    }

    let mut errors = Vec::new();
    if let Some(error) = job.error {
        errors.push(error);
    }
    errors.extend(job.warnings);

    IngestionJob {
        id: job.job_key,
        status: job.status,
        document_id,
        progress: job.progress as f32,
        next_actions,
        errors,
    }
}

pub(crate) fn source_error_to_mcp(error: crate::sources::SourceError) -> McpError {
    let message = error.to_string();
    match error {
        crate::sources::SourceError::InvalidInput { .. } => McpError::invalid_params(message, None),
        crate::sources::SourceError::SourceChanged { .. }
        | crate::sources::SourceError::Fetch { .. } => McpError::internal_error(message, None),
    }
}

pub(crate) fn store_error_to_mcp(error: StoreError) -> McpError {
    McpError::internal_error(error.to_string(), None)
}

pub(crate) fn ingestion_job_error_to_mcp(error: StoreError) -> McpError {
    match error {
        StoreError::MissingIngestionJob(_) => McpError::invalid_params(error.to_string(), None),
        other => McpError::internal_error(other.to_string(), None),
    }
}

pub(crate) fn document_lookup_error_to_mcp(error: StoreError) -> McpError {
    match error {
        StoreError::MissingDocument(_)
        | StoreError::MissingPages { .. }
        | StoreError::InvalidPageRange(_) => McpError::invalid_params(error.to_string(), None),
        other => McpError::internal_error(other.to_string(), None),
    }
}

pub(crate) fn validate_text_page_range(
    page_start: Option<u32>,
    page_end: Option<u32>,
) -> Result<(u32, u32), McpError> {
    let (Some(page_start), Some(page_end)) = (page_start, page_end) else {
        return Err(FoiaSearchError::InvalidRequest(
            "page_start and page_end are required to avoid unbounded full-text retrieval"
                .to_string(),
        )
        .into_mcp_error());
    };
    if page_start == 0 || page_end == 0 {
        return Err(FoiaSearchError::InvalidRequest(
            "page_start and page_end must be one-based".to_string(),
        )
        .into_mcp_error());
    }
    if page_start > page_end {
        return Err(FoiaSearchError::InvalidRequest(
            "page_start must be less than or equal to page_end".to_string(),
        )
        .into_mcp_error());
    }
    if page_end - page_start + 1 > MAX_TEXT_PAGE_RANGE {
        return Err(FoiaSearchError::InvalidRequest(format!(
            "page range is too large; request at most {MAX_TEXT_PAGE_RANGE} pages"
        ))
        .into_mcp_error());
    }
    Ok((page_start, page_end))
}

pub(crate) fn local_document_from_stored(
    document: StoredDocumentMetadata,
) -> Result<LocalDocument, McpError> {
    let metadata_json = serde_json::from_str(&document.metadata_json)
        .map_err(FoiaSearchError::from)
        .map_err(FoiaSearchError::into_mcp_error)?;
    let warnings = source_warnings_from_metadata(&metadata_json);
    Ok(LocalDocument {
        id: document.public_id.clone(),
        document_key: document.document_key.to_string(),
        public_id: document.public_id,
        title: document.title,
        source: document.source,
        source_id: document.source_id,
        date: document.date,
        collection: document.collection,
        record_group: document.record_group,
        description: document.description,
        origin_url: document.origin_url,
        document_url: document.document_url,
        pdf_url: document.pdf_url,
        metadata_json,
        citation_note: document.citation_note,
        terms_note: document.terms_note,
        page_count: document.page_count,
        warnings,
    })
}

pub(crate) fn local_document_text_from_stored(
    document: StoredDocumentMetadata,
    page_start: u32,
    page_end: u32,
    pages: Vec<StoredPageText>,
) -> Result<LocalDocumentText, McpError> {
    let document_key = document.document_key.to_string();
    let metadata_json = serde_json::from_str(&document.metadata_json)
        .map_err(FoiaSearchError::from)
        .map_err(FoiaSearchError::into_mcp_error)?;
    let warnings = source_warnings_from_metadata(&metadata_json);
    let text = pages
        .iter()
        .map(|page| format!("[page {}]\n{}", page.page_number, page.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    let pages = pages
        .into_iter()
        .map(|page| LocalPageText {
            page_number: page.page_number,
            citation: format!("{document_key}#page={}", page.page_number),
            text_source: page.text_source,
            text: page.text,
        })
        .collect();

    Ok(LocalDocumentText {
        document_key,
        public_id: document.public_id,
        title: document.title,
        page_start,
        page_end,
        pages,
        text,
        warnings,
    })
}

pub(crate) fn local_search_hit_from_index(hit: SearchHit) -> LocalSearchHit {
    let warnings = source_warnings_from_metadata_str(&hit.metadata_json);
    LocalSearchHit {
        document_key: hit.document_key.to_string(),
        chunk_id: hit.chunk_id,
        source: hit.source,
        title: hit.title,
        page_start: hit.page_start,
        page_end: hit.page_end,
        score: hit.score,
        snippet: hit.snippet,
        citation_note: hit.citation_note,
        terms_note: hit.terms_note,
        warnings,
    }
}

fn source_warnings_from_metadata_str(metadata_json: &str) -> Vec<String> {
    serde_json::from_str::<Value>(metadata_json)
        .ok()
        .map(|metadata| source_warnings_from_metadata(&metadata))
        .unwrap_or_default()
}

fn source_warnings_from_metadata(metadata: &Value) -> Vec<String> {
    let Some(warning) = metadata
        .get("source_metadata")
        .and_then(|source_metadata| source_metadata.get("source_warning"))
        .or_else(|| metadata.get("source_warning"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|warning| !warning.is_empty())
    else {
        return Vec::new();
    };

    vec![warning.to_owned()]
}
