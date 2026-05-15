use std::{fs, sync::Arc};

use rmcp::{
    handler::server::tool::ToolRouter, handler::server::wrapper::Parameters, model::*, tool,
    tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    config::Config,
    errors::FoiaSearchError,
    index::{FtsSearch, SearchQuery},
    mcp::output::json_result,
    model::{IngestionJob, LocalDocument, LocalSearchHit, PlaceholderResponse, SearchPage},
    sources::{cia::CiaAdapter, SearchOptions, SourceAdapter, SourceError, SourceStatus},
    store::{SqliteStore, StoreError},
};

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchSourceParams {
    #[schemars(description = "Single source to search: cia, nara, govinfo, frus, dtic, or noaa")]
    source: String,
    #[schemars(description = "Research query to send to the source adapter")]
    query: String,
    #[schemars(description = "Opaque pagination cursor returned by a previous search_source call")]
    cursor: Option<String>,
    #[schemars(description = "Maximum records to return. Default 10, maximum 50")]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetSourceRecordParams {
    #[schemars(description = "Source adapter name: cia, nara, govinfo, frus, dtic, or noaa")]
    source: String,
    #[schemars(description = "Source record ID or canonical source URL")]
    id_or_url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IngestDocumentParams {
    #[schemars(description = "Document ID such as cia:CREST-... or nara:123456")]
    document_id: String,
    #[schemars(description = "Force re-fetching source assets even if already cached")]
    force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetIngestionJobParams {
    #[schemars(description = "Stable ingestion job ID returned by ingest_document")]
    job_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchLocalDocumentsParams {
    #[schemars(description = "Keyword query over locally ingested document text and metadata")]
    query: String,
    #[schemars(description = "Optional source filter: cia, nara, govinfo, frus, dtic, or noaa")]
    source: Option<String>,
    #[schemars(description = "Maximum local results to return. Default 10, maximum 100")]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetDocumentParams {
    #[schemars(description = "Local or source-prefixed document ID")]
    document_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetDocumentTextParams {
    #[schemars(description = "Local or source-prefixed document ID")]
    document_id: String,
    #[schemars(description = "First page to return, one-based and inclusive")]
    page_start: Option<u32>,
    #[schemars(description = "Last page to return, one-based and inclusive")]
    page_end: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RefreshDocumentParams {
    #[schemars(
        description = "Local or source-prefixed document ID to re-check against its source"
    )]
    document_id: String,
    #[schemars(description = "Force refresh of remote metadata and assets")]
    force: Option<bool>,
}

#[derive(Clone)]
pub struct FoiaSearchServer {
    tool_router: ToolRouter<Self>,
    config: Arc<Config>,
    sources: Arc<Vec<Arc<dyn SourceAdapter>>>,
}

#[tool_router]
impl FoiaSearchServer {
    pub fn create() -> anyhow::Result<Self> {
        let config = Config::from_env();
        tracing::info!(data_dir = %config.data_dir.display(), "initialized foia-search config");

        Ok(Self {
            tool_router: Self::tool_router(),
            config: Arc::new(config),
            sources: Arc::new(vec![Arc::new(CiaAdapter::default())]),
        })
    }

    #[tool(
        description = "List FOIA/declassified-document sources, their implementation status, and configuration notes. Use this before search_source when choosing where to search."
    )]
    async fn list_sources(&self) -> Result<CallToolResult, McpError> {
        let mut statuses = self.config.source_status();
        for adapter in self.sources.iter() {
            if let Some(status) = statuses
                .iter_mut()
                .find(|status| status.name == adapter.name())
            {
                status.enabled = adapter.status() == SourceStatus::Enabled;
                status.status = "parser_available".to_owned();
                status.note =
                    "Rust parser is available; network fetching is not wired yet.".to_owned();
            }
        }
        json_result(&statuses)
    }

    #[tool(
        description = "Search exactly one external FOIA/declassified-document source and return normalized records with source terms and citation notes. Placeholder scaffold: source adapters are not implemented yet."
    )]
    async fn search_source(
        &self,
        Parameters(params): Parameters<SearchSourceParams>,
    ) -> Result<CallToolResult, McpError> {
        let adapter = self.source_adapter(&params.source)?;
        let limit = params.limit.unwrap_or(10).min(50);
        let options = SearchOptions {
            max_results: limit as usize,
            cursor: params.cursor,
        };

        match adapter.search(&params.query, options).await {
            Ok(page) => json_result(&SearchPage {
                source: page.source.to_owned(),
                query: page.query,
                records: page
                    .records
                    .into_iter()
                    .map(crate::model::SourceRecord::from)
                    .collect(),
                next_cursor: page.next_cursor,
                warnings: page.warnings,
            }),
            Err(error) => Err(source_error_to_mcp(error)),
        }
    }

    #[tool(
        description = "Fetch a normalized record from one source by source ID or URL. Placeholder scaffold: returns a structured not-implemented error until adapters exist."
    )]
    async fn get_source_record(
        &self,
        Parameters(params): Parameters<GetSourceRecordParams>,
    ) -> Result<CallToolResult, McpError> {
        let adapter = self.source_adapter(&params.source)?;
        match adapter.get_record(&params.id_or_url).await {
            Ok(record) => json_result(&crate::model::SourceRecord::from(record)),
            Err(error) => Err(source_error_to_mcp(error)),
        }
    }

    #[tool(
        description = "Start a resumable ingestion job for a source document. Future implementation will download assets, extract/OCR text, persist provenance, and index pages/chunks."
    )]
    async fn ingest_document(
        &self,
        Parameters(params): Parameters<IngestDocumentParams>,
    ) -> Result<CallToolResult, McpError> {
        json_result(&IngestionJob {
            id: format!("placeholder:{}", params.document_id),
            status: "not_started".to_string(),
            document_id: Some(params.document_id),
            progress: 0.0,
            next_actions: vec![
                "Implement store::sqlite job persistence.".to_string(),
                "Implement ingest::pipeline for asset fetch, text extraction, and OCR fallback."
                    .to_string(),
            ],
            errors: vec![format!(
                "Ingestion pipeline is not implemented; force={}.",
                params.force.unwrap_or(false)
            )],
        })
    }

    #[tool(
        description = "Get durable ingestion job status, progress, errors, and next actions by job ID. Placeholder scaffold: job store is not implemented yet."
    )]
    async fn get_ingestion_job(
        &self,
        Parameters(params): Parameters<GetIngestionJobParams>,
    ) -> Result<CallToolResult, McpError> {
        json_result(&IngestionJob {
            id: params.job_id,
            status: "unknown".to_string(),
            document_id: None,
            progress: 0.0,
            next_actions: vec!["Implement durable ingestion job lookup.".to_string()],
            errors: vec!["Ingestion job store is not implemented.".to_string()],
        })
    }

    #[tool(
        description = "Search locally ingested document metadata, page text, and chunks with traceable document/page results. Placeholder scaffold: local index is empty until store/index modules are implemented."
    )]
    async fn search_local_documents(
        &self,
        Parameters(params): Parameters<SearchLocalDocumentsParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(source) = params.source.as_deref() {
            validate_source(source)?;
        }
        let store = self.open_store()?;
        let hits = FtsSearch::new(&store)
            .search(&SearchQuery {
                query: params.query,
                source: params.source,
                limit: i64::from(params.limit.unwrap_or(10).min(100)),
            })
            .map_err(store_error_to_mcp)?
            .into_iter()
            .map(|hit| LocalSearchHit {
                document_key: hit.document_key.to_string(),
                chunk_id: hit.chunk_id,
                source: hit.source,
                title: hit.title,
                page_start: hit.page_start,
                page_end: hit.page_end,
                score: hit.score,
                snippet: hit.snippet,
            })
            .collect::<Vec<_>>();
        json_result(&hits)
    }

    #[tool(
        description = "Get normalized metadata and provenance for a locally ingested document. Placeholder scaffold: document store is not implemented yet."
    )]
    async fn get_document(
        &self,
        Parameters(params): Parameters<GetDocumentParams>,
    ) -> Result<CallToolResult, McpError> {
        json_result(&LocalDocument {
            id: params.document_id,
            title: "Document store not implemented".to_string(),
            source: "unknown".to_string(),
            source_id: "unknown".to_string(),
            page_count: None,
            warnings: vec!["No local document lookup was performed.".to_string()],
        })
    }

    #[tool(
        description = "Get extracted or OCR text for a document, optionally constrained to a one-based inclusive page range. Placeholder scaffold: text store is not implemented yet."
    )]
    async fn get_document_text(
        &self,
        Parameters(params): Parameters<GetDocumentTextParams>,
    ) -> Result<CallToolResult, McpError> {
        if let (Some(start), Some(end)) = (params.page_start, params.page_end) {
            if start > end {
                return Err(FoiaSearchError::InvalidRequest(
                    "page_start must be less than or equal to page_end".to_string(),
                )
                .into_mcp_error());
            }
        }

        let response = PlaceholderResponse {
            status: "not_implemented",
            tool: "get_document_text",
            message: format!(
                "Text lookup is not implemented for document '{}'.",
                params.document_id
            ),
            next_actions: vec![
                "Implement page_text and chunk retrieval in the store layer.".to_string(),
            ],
        };
        json_result(&response)
    }

    #[tool(
        description = "Refresh a locally ingested document from its source, preserving provenance and creating a new ingestion job when assets changed. Placeholder scaffold only."
    )]
    async fn refresh_document(
        &self,
        Parameters(params): Parameters<RefreshDocumentParams>,
    ) -> Result<CallToolResult, McpError> {
        json_result(&IngestionJob {
            id: format!("refresh-placeholder:{}", params.document_id),
            status: "not_started".to_string(),
            document_id: Some(params.document_id),
            progress: 0.0,
            next_actions: vec!["Implement source refresh and changed-asset detection.".to_string()],
            errors: vec![format!(
                "Refresh pipeline is not implemented; force={}.",
                params.force.unwrap_or(false)
            )],
        })
    }
}

impl FoiaSearchServer {
    fn source_adapter(&self, source: &str) -> Result<Arc<dyn SourceAdapter>, McpError> {
        validate_source(source)?;
        self.sources
            .iter()
            .find(|adapter| adapter.name() == source)
            .cloned()
            .ok_or_else(|| {
                FoiaSearchError::SourceNotImplemented {
                    adapter: source.to_owned(),
                }
                .into_mcp_error()
            })
    }

    fn open_store(&self) -> Result<SqliteStore, McpError> {
        let db_dir = self.config.data_dir.join("db");
        fs::create_dir_all(&db_dir).map_err(|err| {
            McpError::internal_error(format!("failed to create data dir: {err}"), None)
        })?;
        SqliteStore::open(db_dir.join("foia.sqlite")).map_err(store_error_to_mcp)
    }
}

#[tool_handler]
impl ServerHandler for FoiaSearchServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Search, ingest, and retrieve FOIA/declassified documents. This Rust scaffold \
                 exposes the planned MCP tool surface with structured placeholder responses while \
                 source adapters, ingestion, store, and index modules are implemented."
                    .into(),
            ),
        }
    }
}

fn validate_source(source: &str) -> Result<(), McpError> {
    const VALID_SOURCES: &[&str] = &["cia", "nara", "govinfo", "frus", "dtic", "noaa"];
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

fn source_error_to_mcp(error: SourceError) -> McpError {
    match error {
        SourceError::InvalidInput { message, .. } => McpError::invalid_params(message, None),
        SourceError::SourceChanged { message, .. } | SourceError::Fetch { message, .. } => {
            McpError::internal_error(message, None)
        }
    }
}

fn store_error_to_mcp(error: StoreError) -> McpError {
    McpError::internal_error(error.to_string(), None)
}
