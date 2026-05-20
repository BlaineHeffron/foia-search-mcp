pub mod cancel;
pub mod chunk;
pub mod download;
mod download_persist;
pub mod executor;
mod executor_async;
#[cfg(test)]
mod executor_cancel_tests;
#[cfg(test)]
mod executor_frus_tests;
#[cfg(test)]
mod executor_govinfo_tests;
#[cfg(test)]
mod executor_nara_tests;
#[cfg(test)]
mod executor_send_tests;
#[cfg(test)]
mod executor_tests;
pub mod jobs;
pub mod ocr;
pub mod ocrmypdf;
#[cfg(test)]
mod ocrmypdf_tests;
pub mod pdf;
pub mod pdf_text;
#[cfg(test)]
mod pdf_text_tests;
pub mod pdftotext;
pub mod pipeline;
pub(crate) mod process;
pub mod reconcile;
pub(crate) mod reconcile_compare;
mod reconcile_repair;
mod reconcile_repair_apply;
#[cfg(test)]
mod reconcile_repair_apply_support_tests;
#[cfg(test)]
mod reconcile_repair_apply_tests;
#[cfg(test)]
mod reconcile_repair_tests;
#[cfg(test)]
mod reconcile_tests;
pub mod redirect;
pub mod source_plan;
#[cfg(test)]
mod source_plan_asset_tests;
pub(crate) mod source_resolution;
pub mod worker;
#[cfg(test)]
mod worker_cancel_tests;
pub(crate) mod worker_ocr;
#[cfg(test)]
mod worker_send_tests;

pub use cancel::{
    ensure_not_cancelled, CancellationCheckpoint, CancellationHandle, CancellationSignal,
    CancellationToken, NeverCancel,
};
pub use chunk::{chunk_pages, Chunk, ChunkOptions, PageText};
pub use download::{
    store_cache_policy, AssetDownloadRequest, AssetDownloader, DownloadCacheStatus, DownloadError,
    DownloadedAsset,
};
pub use executor::{ExecutorError, ExecutorJobOutcome, QueuedIngestionExecutor};
pub use jobs::{IngestionJobLease, IngestionJobRecord};
pub use ocr::{NoopOcrExtractor, OcrBackend, OcrBackendConfig, OcrFallbackMode, OcrFallbackPolicy};
pub use ocrmypdf::{OcrmypdfConfig, OcrmypdfExtractor};
pub use pdf::{ExtractedText, TextExtraction, TextExtractor, TextFileExtractor};
pub use pdf_text::{select_pdf_text, SelectedPdfText};
pub use pdftotext::{PdftotextConfig, PdftotextExtractor};
pub use pipeline::{ingest_text_file, IngestDocument, IngestError, IngestOutcome};
pub use reconcile_repair::{
    plan_derived_artifact_repairs, DerivedArtifactRepairAction, DerivedArtifactRepairPlan,
    DerivedArtifactRewriteReason,
};
pub use reconcile_repair_apply::{
    apply_derived_artifact_repairs, DerivedArtifactApplyReport, RepairApplyError,
};
pub use redirect::{RedirectFollowPolicy, RedirectPolicy};
pub use source_plan::{
    plan_source_ingestion, PlannedSourceAsset, SourceIngestionPlan, SourcePlanError,
    SourcePlanMetadata,
};
pub use worker::{
    IngestionWorkerHandle, IngestionWorkerKick, QueuedIngestionWorker, WorkerError, WorkerKickError,
};
