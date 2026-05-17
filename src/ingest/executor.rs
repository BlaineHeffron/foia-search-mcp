use crate::ingest::{
    cancel::{ensure_not_cancelled, CancellationCheckpoint, CancellationSignal, NeverCancel},
    download::store_cache_policy,
    ocr::{NoopOcrExtractor, OcrFallbackPolicy},
    pdf_text::select_pdf_text_with_cancel,
    pipeline::ingest_extracted_text,
    source_plan::plan_source_ingestion,
    AssetDownloadRequest, AssetDownloader, ChunkOptions, DownloadError, IngestError,
    IngestionJobLease, IngestionJobRecord, SourcePlanError, TextExtraction, TextExtractor,
    TextFileExtractor,
};
use crate::sources::{SourceAdapter, SourceAsset, SourceAssetRole, SourceError};
use crate::store::{
    AssetInput, AssetRole, CacheStore, ContentAddressedStore, SqliteStore, StoreError,
};
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct QueuedIngestionExecutor {
    owner: String,
    lease_seconds: u32,
    sources: Vec<Arc<dyn SourceAdapter>>,
    downloader: AssetDownloader,
    chunk_options: ChunkOptions,
    force_download: bool,
    ocr_policy: OcrFallbackPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutorJobOutcome {
    pub job_key: String,
    pub document_key: String,
    pub page_count: usize,
    pub chunk_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum ExecutorError {
    Store(StoreError),
    MissingSourceAdapter { source: String },
    MissingJobLocator { job_key: String },
    Source(SourceError),
    Plan(SourcePlanError),
    Download(DownloadError),
    Ingest(IngestError),
    AssetSizeOverflow { size_bytes: u64 },
    Cancelled { checkpoint: CancellationCheckpoint },
}

impl QueuedIngestionExecutor {
    pub fn new(
        owner: impl Into<String>,
        sources: Vec<Arc<dyn SourceAdapter>>,
    ) -> Result<Self, ExecutorError> {
        Ok(Self {
            owner: owner.into(),
            lease_seconds: 300,
            sources,
            downloader: AssetDownloader::new()?,
            chunk_options: ChunkOptions::default(),
            force_download: false,
            ocr_policy: OcrFallbackPolicy::off(),
        })
    }

    pub fn with_downloader(mut self, downloader: AssetDownloader) -> Self {
        self.downloader = downloader;
        self
    }

    pub fn with_chunk_options(mut self, chunk_options: ChunkOptions) -> Self {
        self.chunk_options = chunk_options;
        self
    }

    pub fn with_force_download(mut self, force_download: bool) -> Self {
        self.force_download = force_download;
        self
    }

    pub fn with_ocr_policy(mut self, ocr_policy: OcrFallbackPolicy) -> Self {
        self.ocr_policy = ocr_policy;
        self
    }

    pub async fn run_next(
        &self,
        store: &mut SqliteStore,
        files: &ContentAddressedStore,
        pdf_extractor: &dyn TextExtractor,
    ) -> Result<Option<ExecutorJobOutcome>, ExecutorError> {
        self.run_next_with_ocr(store, files, pdf_extractor, &NoopOcrExtractor)
            .await
    }

    pub async fn run_next_with_ocr(
        &self,
        store: &mut SqliteStore,
        files: &ContentAddressedStore,
        pdf_extractor: &dyn TextExtractor,
        ocr_extractor: &dyn TextExtractor,
    ) -> Result<Option<ExecutorJobOutcome>, ExecutorError> {
        self.run_next_with_ocr_and_cancel(store, files, pdf_extractor, ocr_extractor, &NeverCancel)
            .await
    }

    pub async fn run_next_with_ocr_and_cancel(
        &self,
        store: &mut SqliteStore,
        files: &ContentAddressedStore,
        pdf_extractor: &dyn TextExtractor,
        ocr_extractor: &dyn TextExtractor,
        cancellation: &dyn CancellationSignal,
    ) -> Result<Option<ExecutorJobOutcome>, ExecutorError> {
        let lease = self.lease(store)?;
        let Some(job) = store.claim_next_ingestion_job(&lease)? else {
            return Ok(None);
        };

        let result = match self.check_cancellation(cancellation, CancellationCheckpoint::AfterClaim)
        {
            Ok(()) => {
                self.execute_claimed_job(
                    store,
                    files,
                    pdf_extractor,
                    ocr_extractor,
                    cancellation,
                    &job,
                )
                .await
            }
            Err(error) => Err(error),
        };

        match result {
            Ok(outcome) => {
                store.complete_ingestion_job(&job.job_key, &self.owner)?;
                Ok(Some(outcome))
            }
            Err(ExecutorError::Cancelled { checkpoint }) => {
                let _ = store.interrupt_ingestion_job(
                    &job.job_key,
                    &self.owner,
                    Some(&format!(
                        "ingestion interrupted by cancellation {checkpoint}"
                    )),
                    Some(checkpoint.next_action()),
                );
                Err(ExecutorError::Cancelled { checkpoint })
            }
            Err(error) => {
                let message = error.to_string();
                let _ = store.fail_ingestion_job(
                    &job.job_key,
                    &self.owner,
                    &message,
                    Some("Ingestion failed deterministically; inspect error, fix source or extractor configuration, then requeue or refresh."),
                );
                Err(error)
            }
        }
    }

    async fn execute_claimed_job(
        &self,
        store: &mut SqliteStore,
        files: &ContentAddressedStore,
        pdf_extractor: &dyn TextExtractor,
        ocr_extractor: &dyn TextExtractor,
        cancellation: &dyn CancellationSignal,
        job: &IngestionJobRecord,
    ) -> Result<ExecutorJobOutcome, ExecutorError> {
        let adapter = self.source_adapter(&job.source)?;
        let id_or_url = job
            .source_id
            .as_deref()
            .or(job.target_url.as_deref())
            .ok_or_else(|| ExecutorError::MissingJobLocator {
                job_key: job.job_key.clone(),
            })?;

        store.mark_ingestion_job_stage(
            &job.job_key,
            &self.owner,
            "resolving_source_record",
            0.10,
            Some("Resolving source record through configured adapter."),
        )?;
        let mut record = adapter.get_record(id_or_url).await?;
        record.attachments = adapter.list_assets(&record).await?;
        self.check_cancellation(cancellation, CancellationCheckpoint::AfterSourceResolution)?;

        store.mark_ingestion_job_stage(
            &job.job_key,
            &self.owner,
            "planning_ingestion",
            0.20,
            Some("Selecting ingestible asset and building normalized document plan."),
        )?;
        let plan = plan_source_ingestion(&record, adapter.cache_policy())?;
        self.check_cancellation(cancellation, CancellationCheckpoint::AfterPlanning)?;
        let source_asset = planned_asset_to_source_asset(&plan.asset);
        let cache_policy = store_cache_policy(plan.cache_policy.clone());

        store.mark_ingestion_job_stage(
            &job.job_key,
            &self.owner,
            "downloading_asset",
            0.35,
            Some("Downloading selected asset with bounded size and explicit redirect policy."),
        )?;
        self.check_cancellation(cancellation, CancellationCheckpoint::BeforeDownload)?;
        let request = AssetDownloadRequest {
            source: adapter.name(),
            asset: &source_asset,
            cache_policy,
            redirect_policy: adapter.redirect_policy(),
            force: self.force_download,
        };
        let cached = {
            let cache = CacheStore::new(store);
            self.downloader.load_cached_entry(&cache, &request)?
        };
        let prepared = self
            .downloader
            .download_http(files, request, cached)
            .await?;
        let downloaded = {
            let cache = CacheStore::new(store);
            self.downloader
                .persist_prepared_download(&cache, prepared)?
        };
        self.check_cancellation(cancellation, CancellationCheckpoint::AfterDownload)?;

        store.mark_ingestion_job_stage(
            &job.job_key,
            &self.owner,
            "extracting_text",
            0.60,
            Some("Extracting page text and building chunks."),
        )?;
        self.check_cancellation(cancellation, CancellationCheckpoint::BeforeExtraction)?;
        let outcome = if plan.asset.role == SourceAssetRole::Pdf {
            let selected = select_pdf_text_with_cancel(
                &downloaded.path,
                pdf_extractor,
                ocr_extractor,
                self.ocr_policy,
                &|| cancellation.is_cancelled(),
            )
            .map_err(|error| match error {
                TextExtraction::Cancelled { .. } => ExecutorError::Cancelled {
                    checkpoint: CancellationCheckpoint::DuringExtraction,
                },
                error => ExecutorError::Ingest(IngestError::from(error)),
            })?;
            store.mark_ingestion_job_stage(
                &job.job_key,
                &self.owner,
                "persisting_document",
                0.80,
                Some("Persisting normalized document, pages, chunks, and local index rows."),
            )?;
            self.check_cancellation(cancellation, CancellationCheckpoint::BeforePersistence)?;
            ingest_extracted_text(
                store,
                plan.document,
                &self.chunk_options,
                selected.extracted,
                Some(selected.text_source),
            )?
        } else {
            let extracted = TextFileExtractor
                .extract_pages(&downloaded.path)
                .map_err(IngestError::from)?;
            store.mark_ingestion_job_stage(
                &job.job_key,
                &self.owner,
                "persisting_document",
                0.80,
                Some("Persisting normalized document, pages, chunks, and local index rows."),
            )?;
            self.check_cancellation(cancellation, CancellationCheckpoint::BeforePersistence)?;
            ingest_extracted_text(store, plan.document, &self.chunk_options, extracted, None)?
        };

        for warning in &outcome.warnings {
            store.record_ingestion_job_warning(&job.job_key, &self.owner, warning)?;
        }
        store.link_ingestion_job_document(&job.job_key, &self.owner, &outcome.document_key)?;

        store.mark_ingestion_job_stage(
            &job.job_key,
            &self.owner,
            "persisting_asset",
            0.90,
            Some("Persisting asset provenance after successful document/page/chunk upsert."),
        )?;
        self.check_cancellation(
            cancellation,
            CancellationCheckpoint::BeforeAssetProvenanceWrite,
        )?;
        store.add_asset(&AssetInput {
            document_key: outcome.document_key.clone(),
            asset_url: downloaded.asset_url,
            mime_type: downloaded.mime_type,
            role: asset_role(downloaded.role),
            sha256: Some(downloaded.sha256),
            size_bytes: Some(asset_size(downloaded.size_bytes)?),
            etag: downloaded.etag,
            last_modified: downloaded.last_modified,
            fetched_at: Some(sqlite_now(store)?),
            cache_policy: Some(cache_policy_name(downloaded.cache_policy).to_owned()),
        })?;

        Ok(ExecutorJobOutcome {
            job_key: job.job_key.clone(),
            document_key: outcome.document_key.to_string(),
            page_count: outcome.page_count,
            chunk_count: outcome.chunk_count,
            warnings: outcome.warnings,
        })
    }

    fn check_cancellation(
        &self,
        cancellation: &dyn CancellationSignal,
        checkpoint: CancellationCheckpoint,
    ) -> Result<(), ExecutorError> {
        ensure_not_cancelled(cancellation, checkpoint)
            .map_err(|checkpoint| ExecutorError::Cancelled { checkpoint })
    }

    fn source_adapter(&self, source: &str) -> Result<Arc<dyn SourceAdapter>, ExecutorError> {
        self.sources
            .iter()
            .find(|adapter| adapter.name() == source)
            .cloned()
            .ok_or_else(|| ExecutorError::MissingSourceAdapter {
                source: source.to_owned(),
            })
    }

    fn lease(&self, store: &SqliteStore) -> Result<IngestionJobLease, StoreError> {
        let (now, expires_at) = sqlite_now_and_expiry(store, self.lease_seconds)?;
        Ok(IngestionJobLease {
            owner: self.owner.clone(),
            now,
            expires_at,
        })
    }
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(err) => write!(f, "{err}"),
            Self::MissingSourceAdapter { source } => {
                write!(f, "no source adapter registered for queued source {source}")
            }
            Self::MissingJobLocator { job_key } => {
                write!(
                    f,
                    "queued ingestion job {job_key} has no source_id or target_url"
                )
            }
            Self::Source(err) => write!(f, "{err}"),
            Self::Plan(err) => write!(f, "{err}"),
            Self::Download(err) => write!(f, "{err}"),
            Self::Ingest(err) => write!(f, "{err}"),
            Self::AssetSizeOverflow { size_bytes } => {
                write!(
                    f,
                    "downloaded asset size does not fit SQLite integer: {size_bytes}"
                )
            }
            Self::Cancelled { checkpoint } => {
                write!(f, "ingestion cancelled {checkpoint}")
            }
        }
    }
}

impl std::error::Error for ExecutorError {}

impl From<StoreError> for ExecutorError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

impl From<SourceError> for ExecutorError {
    fn from(err: SourceError) -> Self {
        Self::Source(err)
    }
}

impl From<SourcePlanError> for ExecutorError {
    fn from(err: SourcePlanError) -> Self {
        Self::Plan(err)
    }
}

impl From<DownloadError> for ExecutorError {
    fn from(err: DownloadError) -> Self {
        Self::Download(err)
    }
}

impl From<IngestError> for ExecutorError {
    fn from(err: IngestError) -> Self {
        Self::Ingest(err)
    }
}

fn planned_asset_to_source_asset(asset: &crate::ingest::PlannedSourceAsset) -> SourceAsset {
    SourceAsset {
        asset_url: asset.url.clone(),
        label: asset.label.clone(),
        mime_type: asset.mime_type.clone(),
        role: asset.role.clone(),
    }
}

fn asset_role(role: SourceAssetRole) -> AssetRole {
    match role {
        SourceAssetRole::Pdf => AssetRole::Pdf,
        SourceAssetRole::Html => AssetRole::Html,
        SourceAssetRole::OcrText => AssetRole::OcrText,
        SourceAssetRole::Transcript => AssetRole::Transcript,
        SourceAssetRole::Image => AssetRole::Image,
        SourceAssetRole::Other => AssetRole::Other,
    }
}

fn asset_size(size_bytes: u64) -> Result<i64, ExecutorError> {
    i64::try_from(size_bytes).map_err(|_| ExecutorError::AssetSizeOverflow { size_bytes })
}

fn cache_policy_name(policy: crate::store::CachePolicy) -> &'static str {
    match policy {
        crate::store::CachePolicy::RespectSourceHeaders => "respect_source_headers",
        crate::store::CachePolicy::DoNotPersist => "do_not_persist",
    }
}

fn sqlite_now(store: &SqliteStore) -> Result<String, StoreError> {
    store
        .connection()
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
            row.get(0)
        })
        .map_err(StoreError::from)
}

fn sqlite_now_and_expiry(
    store: &SqliteStore,
    lease_seconds: u32,
) -> Result<(String, String), StoreError> {
    store
        .connection()
        .query_row(
            "
            SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                   strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1)
            ",
            [format!("+{lease_seconds} seconds")],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(StoreError::from)
}
