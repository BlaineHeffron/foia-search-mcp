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
    ingest::IngestionWorkerKick,
    mcp::output::json_result,
    mcp::{
        fts_repair,
        ingestion::enqueue_ingestion_job,
        repair,
        support::{
            document_lookup_error_to_mcp, ingestion_job_error_to_mcp, ingestion_job_from_stored,
            local_document_from_stored, local_document_text_from_stored, source_error_to_mcp,
            store_error_to_mcp, validate_source, validate_text_page_range,
        },
    },
    model::{LocalSearchHit, SearchPage},
    sources::{SearchOptions, SourceAdapter, SourceStatus},
    store::{ContentAddressedStore, SqliteStore},
};

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchSourceParams {
    #[schemars(
        description = "Single source to search: aaro, cia, nara, govinfo, pursue, doj_epstein, doj_foia, fbi_vault, frus, dtic, dia, noaa, nsa, osd_joint_staff, or state"
    )]
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
    #[schemars(
        description = "Source adapter name: aaro, cia, nara, govinfo, pursue, doj_epstein, doj_foia, fbi_vault, frus, dtic, dia, noaa, nsa, osd_joint_staff, or state"
    )]
    source: String,
    #[schemars(description = "Source record ID or canonical source URL")]
    id_or_url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IngestDocumentParams {
    #[schemars(
        description = "Document ID such as aaro:UAP-Records/history-and-origin-of-kona-blue, cia:CREST-..., nara:123456, govinfo:PKG, pursue:release-01:record, doj_epstein:data-set-1-files, doj_foia:criminal-division, fbi_vault:rosenberg-case/mark-page, nsa:Helpful-Links/NSA-FOIA/Reading-Room/FOIA-Handbook, osd_joint_staff:Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff, or state:FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213"
    )]
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
    #[schemars(
        description = "Optional source filter: aaro, cia, nara, govinfo, pursue, doj_epstein, doj_foia, fbi_vault, frus, dtic, dia, noaa, nsa, osd_joint_staff, or state"
    )]
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
struct ReportDerivedArtifactDriftParams {
    #[schemars(description = "Local or source-prefixed document ID to inspect")]
    document_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PlanDerivedArtifactRepairsParams {
    #[schemars(description = "Local or source-prefixed document ID to inspect")]
    document_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ApplyDerivedArtifactRepairsParams {
    #[schemars(description = "Local or source-prefixed document ID to repair")]
    document_id: String,
    #[schemars(
        description = "Explicit confirmation string: apply derived artifact repairs for <document_id>"
    )]
    confirmation: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReportSqliteFtsDriftParams {}

#[derive(Debug, Deserialize, JsonSchema)]
struct PlanSqliteFtsRepairsParams {}

#[derive(Debug, Deserialize, JsonSchema)]
struct ApplySqliteFtsRepairsParams {
    #[schemars(description = "Explicit confirmation string: apply sqlite fts repairs")]
    confirmation: String,
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
    ingestion_worker: Option<IngestionWorkerKick>,
}

#[tool_router]
impl FoiaSearchServer {
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
                if status.enabled {
                    status.status = "enabled".to_owned();
                }
                status.note =
                    crate::mcp::sources::list_sources_note(adapter.name(), status.enabled)
                        .unwrap_or_else(|| status.note.clone());
            }
        }
        json_result(&statuses)
    }

    #[tool(
        description = "Search exactly one external FOIA/declassified-document source and return normalized records with source terms and citation notes. AARO, Army FOIA Reading Room, Navy FOIA Reading Room, CIA, GovInfo, PURSUE, DOJ Epstein, DOJ component FOIA, FBI Vault, FRUS, NOAA, NSA, State Department Virtual Reading Room, DIA FOIA Electronic Reading Room, and OSD/Joint Staff FOIA Reading Room are wired for public HTTP search; DTIC is wired in accession/official-URL tracer mode with fragility warnings; NARA is wired for API-key Catalog search when configured."
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
        description = "Fetch a normalized record from one source by source ID or URL. AARO, Army FOIA Reading Room, Navy FOIA Reading Room, CIA, GovInfo, PURSUE, DOJ Epstein, DOJ component FOIA, FBI Vault, FRUS, NOAA, NSA, State Department Virtual Reading Room, DIA FOIA Electronic Reading Room, and OSD/Joint Staff FOIA Reading Room are wired for public HTTP fetch; DTIC is wired in accession/official-URL tracer mode with fragility warnings; NARA is wired for API-key Catalog fetch when configured."
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

    #[tool(description = "Start a resumable queued ingestion job for a source document.")]
    async fn ingest_document(
        &self,
        Parameters(params): Parameters<IngestDocumentParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut store = self.open_store()?;
        let job = enqueue_ingestion_job(
            &mut store,
            "ingest",
            "ingestion",
            &params.document_id,
            params.force.unwrap_or(false),
        )?;
        self.kick_ingestion_worker();
        json_result(&ingestion_job_from_stored(job))
    }

    #[tool(
        description = "Get durable ingestion job status, progress, errors, and next actions by job ID."
    )]
    async fn get_ingestion_job(
        &self,
        Parameters(params): Parameters<GetIngestionJobParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.open_store()?;
        let job = store
            .get_ingestion_job_by_key(&params.job_id)
            .map_err(ingestion_job_error_to_mcp)?;
        json_result(&ingestion_job_from_stored(job))
    }

    #[tool(
        description = "Search locally ingested document metadata, page text, and chunks with traceable document/page results."
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
        description = "Get normalized metadata and provenance for a locally ingested document by public ID or local document_key."
    )]
    async fn get_document(
        &self,
        Parameters(params): Parameters<GetDocumentParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.open_store()?;
        let document = store
            .get_document_metadata(&params.document_id)
            .map_err(document_lookup_error_to_mcp)?;
        let response = local_document_from_stored(document)?;
        json_result(&response)
    }

    #[tool(
        description = "Get extracted or OCR text for a document by public ID or local document_key, constrained to a required one-based inclusive page range of at most 50 pages."
    )]
    async fn get_document_text(
        &self,
        Parameters(params): Parameters<GetDocumentTextParams>,
    ) -> Result<CallToolResult, McpError> {
        let (page_start, page_end) = validate_text_page_range(params.page_start, params.page_end)?;
        let store = self.open_store()?;
        let document = store
            .get_document_metadata(&params.document_id)
            .map_err(document_lookup_error_to_mcp)?;
        let pages = store
            .get_page_text(&params.document_id, page_start, page_end)
            .map_err(document_lookup_error_to_mcp)?;
        let response = local_document_text_from_stored(document, page_start, page_end, pages);
        json_result(&response)
    }

    #[tool(
        description = "Report derived text and OCR artifact drift for one local document without writing anything."
    )]
    async fn report_derived_artifact_drift(
        &self,
        Parameters(params): Parameters<ReportDerivedArtifactDriftParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.open_store()?;
        let files = self.open_files();
        let response = repair::report_derived_artifact_drift(&store, &files, &params.document_id)
            .map_err(|error| error.into_mcp_error())?;
        json_result(&response)
    }

    #[tool(
        description = "Plan derived artifact repairs for one local document without writing anything."
    )]
    async fn plan_derived_artifact_repairs(
        &self,
        Parameters(params): Parameters<PlanDerivedArtifactRepairsParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.open_store()?;
        let files = self.open_files();
        let response = repair::plan_derived_artifact_repairs(&store, &files, &params.document_id)
            .map_err(|error| error.into_mcp_error())?;
        json_result(&response)
    }

    #[tool(
        description = "Apply derived artifact repairs for one local document. This requires explicit confirmation string 'apply derived artifact repairs for <document_id>'."
    )]
    async fn apply_derived_artifact_repairs(
        &self,
        Parameters(params): Parameters<ApplyDerivedArtifactRepairsParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.open_store()?;
        let files = self.open_files();
        let response = repair::apply_derived_artifact_repairs(
            &store,
            &files,
            &params.document_id,
            &params.confirmation,
        )
        .map_err(|error| error.into_mcp_error())?;
        json_result(&response)
    }

    #[tool(description = "Report SQLite chunk_fts index drift without writing anything.")]
    async fn report_sqlite_fts_drift(
        &self,
        Parameters(_params): Parameters<ReportSqliteFtsDriftParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.open_store()?;
        let response =
            fts_repair::report_sqlite_fts_drift(&store).map_err(|error| error.into_mcp_error())?;
        json_result(&response)
    }

    #[tool(description = "Plan SQLite chunk_fts index repairs without writing anything.")]
    async fn plan_sqlite_fts_repairs(
        &self,
        Parameters(_params): Parameters<PlanSqliteFtsRepairsParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.open_store()?;
        let response =
            fts_repair::plan_sqlite_fts_repairs(&store).map_err(|error| error.into_mcp_error())?;
        json_result(&response)
    }

    #[tool(
        description = "Apply SQLite chunk_fts index repairs. This requires explicit confirmation string 'apply sqlite fts repairs'. Orphaned chunk_fts rows are skipped for manual review."
    )]
    async fn apply_sqlite_fts_repairs(
        &self,
        Parameters(params): Parameters<ApplySqliteFtsRepairsParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.open_store()?;
        let response = fts_repair::apply_sqlite_fts_repairs(&store, &params.confirmation)
            .map_err(|error| error.into_mcp_error())?;
        json_result(&response)
    }

    #[tool(
        description = "Refresh a locally ingested document from its source by creating a durable queued ingestion job."
    )]
    async fn refresh_document(
        &self,
        Parameters(params): Parameters<RefreshDocumentParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut store = self.open_store()?;
        let job = enqueue_ingestion_job(
            &mut store,
            "refresh",
            "refresh",
            &params.document_id,
            params.force.unwrap_or(false),
        )?;
        self.kick_ingestion_worker();
        json_result(&ingestion_job_from_stored(job))
    }
}

impl FoiaSearchServer {
    pub(crate) fn from_parts(
        config: Arc<Config>,
        sources: Arc<Vec<Arc<dyn SourceAdapter>>>,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            config,
            sources,
            ingestion_worker: None,
        }
    }

    pub(crate) fn with_ingestion_worker(mut self, worker: IngestionWorkerKick) -> Self {
        self.ingestion_worker = Some(worker);
        self
    }

    fn source_adapter(&self, source: &str) -> Result<Arc<dyn SourceAdapter>, McpError> {
        validate_source(source)?;
        self.sources
            .iter()
            .find(|adapter| adapter.name() == source)
            .cloned()
            .ok_or_else(|| {
                FoiaSearchError::SourceUnavailable {
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

    fn open_files(&self) -> ContentAddressedStore {
        ContentAddressedStore::new(&self.config.data_dir)
    }

    fn kick_ingestion_worker(&self) {
        if let Some(worker) = &self.ingestion_worker {
            if let Err(error) = worker.kick() {
                tracing::warn!(error = %error, "queued ingestion worker kick failed");
            }
        }
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
                "Search, ingest, and retrieve FOIA/declassified documents. The Rust server \
                 exposes source search, durable ingestion jobs, local document search, and \
                 document text retrieval backed by SQLite storage and source provenance."
                    .into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::ingestion::{parse_document_locator, queued_next_action};
    use crate::mcp::support::MAX_TEXT_PAGE_RANGE;
    use crate::store::{StoredDocumentMetadata, StoredIngestionJob, StoredPageText};

    #[test]
    fn text_page_range_requires_explicit_bounds() {
        assert!(validate_text_page_range(None, None).is_err());
        assert!(validate_text_page_range(Some(1), None).is_err());
        assert!(validate_text_page_range(None, Some(1)).is_err());
    }

    #[test]
    fn text_page_range_rejects_zero_inverted_and_oversized_ranges() {
        assert!(validate_text_page_range(Some(0), Some(1)).is_err());
        assert!(validate_text_page_range(Some(2), Some(1)).is_err());
        assert!(validate_text_page_range(Some(1), Some(MAX_TEXT_PAGE_RANGE + 1)).is_err());
    }

    #[test]
    fn text_page_range_accepts_inclusive_bounded_ranges() {
        assert_eq!(
            validate_text_page_range(Some(3), Some(5)).expect("valid range"),
            (3, 5)
        );
        assert_eq!(
            validate_text_page_range(Some(1), Some(MAX_TEXT_PAGE_RANGE)).expect("max valid range"),
            (1, MAX_TEXT_PAGE_RANGE)
        );
    }

    #[test]
    fn source_validation_accepts_known_sources_and_rejects_unknown_sources() {
        for source in [
            "aaro",
            "army",
            "cia",
            "nara",
            "navy",
            "govinfo",
            "pursue",
            "doj_epstein",
            "doj_foia",
            "fbi_vault",
            "frus",
            "dtic",
            "dia",
            "noaa",
            "nsa",
            "osd_joint_staff",
            "state",
        ] {
            assert!(validate_source(source).is_ok(), "{source} should be valid");
        }

        let error = validate_source("state-dept").expect_err("unknown source should fail");
        assert!(error.message.contains("invalid source"));
        let expected_sources = crate::mcp::sources::VALID_SOURCES.join(", ");
        assert!(error.message.contains(&expected_sources));
    }

    #[test]
    fn document_locator_requires_source_prefix_and_source_id() {
        let missing_prefix = locator_error("CREST-123");
        assert!(missing_prefix
            .message
            .contains("document_id must use '<source>:<source_id>' format"));

        let missing_source_id = locator_error("cia:   ");
        assert!(missing_source_id
            .message
            .contains("document_id source_id must not be empty"));

        let invalid_source = locator_error("state-dept:123");
        assert!(invalid_source.message.contains("invalid source"));
    }

    #[test]
    fn document_locator_preserves_valid_source_and_id() {
        let locator = parse_document_locator("cia:CREST-123").expect("valid locator");

        assert_eq!(locator.source, "cia");
        assert_eq!(locator.source_id, "CREST-123");
    }

    #[test]
    fn ingestion_job_response_includes_document_id_next_actions_and_errors() {
        let response = ingestion_job_from_stored(StoredIngestionJob {
            job_key: "ingest:cia:CREST-123".to_owned(),
            source: "cia".to_owned(),
            source_id: Some("CREST-123".to_owned()),
            target_url: None,
            status: "queued".to_owned(),
            stage: "queued".to_owned(),
            progress: 0.25,
            error: Some("previous transient failure".to_owned()),
            warnings: vec!["OCR quality warning".to_owned()],
            next_action: Some(queued_next_action("ingestion", true)),
        });

        assert_eq!(response.id, "ingest:cia:CREST-123");
        assert_eq!(response.document_id.as_deref(), Some("cia:CREST-123"));
        assert_eq!(response.status, "queued");
        assert_eq!(response.progress, 0.25);
        assert!(response
            .next_actions
            .iter()
            .any(|action| action.contains("force=true")));
        assert!(response
            .next_actions
            .iter()
            .any(|action| action.contains("background worker")));
        assert_eq!(
            response.errors,
            vec![
                "previous transient failure".to_owned(),
                "OCR quality warning".to_owned()
            ]
        );
    }

    #[test]
    fn ingestion_job_response_falls_back_to_target_url_and_stage_action() {
        let response = ingestion_job_from_stored(StoredIngestionJob {
            job_key: "ingest:https://example.test/doc".to_owned(),
            source: "cia".to_owned(),
            source_id: None,
            target_url: Some("https://example.test/doc".to_owned()),
            status: "running".to_owned(),
            stage: "extracting_text".to_owned(),
            progress: 0.5,
            error: None,
            warnings: Vec::new(),
            next_action: None,
        });

        assert_eq!(
            response.document_id.as_deref(),
            Some("https://example.test/doc")
        );
        assert_eq!(
            response.next_actions,
            vec!["Current stage is 'extracting_text'.".to_owned()]
        );
        assert!(response.errors.is_empty());
    }

    #[test]
    fn local_document_response_parses_metadata_json() {
        let document = stored_document_metadata(r#"{"classification":"declassified"}"#);
        let response = local_document_from_stored(document).expect("valid metadata JSON");

        assert_eq!(response.id, "cia:CREST-lookup");
        assert_eq!(response.document_key, "doc_cia_lookup");
        assert_eq!(response.source, "cia");
        assert_eq!(response.page_count, 2);
        assert_eq!(response.metadata_json["classification"], "declassified");
    }

    #[test]
    fn local_document_response_rejects_invalid_metadata_json() {
        let error = local_document_from_stored(stored_document_metadata("not-json"))
            .expect_err("invalid metadata JSON should fail");

        assert!(error.message.contains("serialization failed"));
    }

    #[test]
    fn local_document_text_response_includes_page_citations_and_combined_text() {
        let response = local_document_text_from_stored(
            stored_document_metadata(r#"{"classification":"declassified"}"#),
            1,
            2,
            vec![
                StoredPageText {
                    page_number: 1,
                    text: "Page one text".to_owned(),
                    text_source: "embedded_pdf_text".to_owned(),
                },
                StoredPageText {
                    page_number: 2,
                    text: "Page two text".to_owned(),
                    text_source: "local_ocr".to_owned(),
                },
            ],
        );

        assert_eq!(response.document_key, "doc_cia_lookup");
        assert_eq!(response.public_id, "cia:CREST-lookup");
        assert_eq!(response.page_start, 1);
        assert_eq!(response.page_end, 2);
        assert_eq!(response.pages.len(), 2);
        assert_eq!(response.pages[0].citation, "doc_cia_lookup#page=1");
        assert_eq!(response.pages[1].citation, "doc_cia_lookup#page=2");
        assert!(response.text.contains("[page 1]\nPage one text"));
        assert!(response.text.contains("[page 2]\nPage two text"));
    }

    fn stored_document_metadata(metadata_json: &str) -> StoredDocumentMetadata {
        StoredDocumentMetadata {
            id: 1,
            public_id: "cia:CREST-lookup".to_owned(),
            document_key: crate::store::DocumentKey::new("doc_cia_lookup")
                .expect("fixture document key should be valid"),
            source: "cia".to_owned(),
            source_id: "CREST-lookup".to_owned(),
            title: "Lookup Test".to_owned(),
            date: Some("1961-01-01".to_owned()),
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("A local lookup fixture".to_owned()),
            origin_url: Some("https://example.test/origin".to_owned()),
            document_url: Some("https://example.test/document".to_owned()),
            pdf_url: Some("https://example.test/document.pdf".to_owned()),
            metadata_json: metadata_json.to_owned(),
            citation_note: Some("Cite page numbers from local OCR.".to_owned()),
            terms_note: Some("Public domain source terms.".to_owned()),
            page_count: 2,
        }
    }

    fn locator_error(document_id: &str) -> McpError {
        match parse_document_locator(document_id) {
            Ok(_) => panic!("{document_id} should fail validation"),
            Err(error) => error,
        }
    }
}
