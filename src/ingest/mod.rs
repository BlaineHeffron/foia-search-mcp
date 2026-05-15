pub mod chunk;
pub mod download;
pub mod jobs;
pub mod pdf;
pub mod pdftotext;
pub mod pipeline;
pub mod source_plan;

pub use chunk::{chunk_pages, Chunk, ChunkOptions, PageText};
pub use download::{
    store_cache_policy, AssetDownloadRequest, AssetDownloader, DownloadCacheStatus, DownloadError,
    DownloadedAsset,
};
pub use pdf::{ExtractedText, TextExtraction, TextExtractor, TextFileExtractor};
pub use pdftotext::{PdftotextConfig, PdftotextExtractor};
pub use pipeline::{ingest_text_file, IngestDocument, IngestError, IngestOutcome};
pub use source_plan::{
    plan_source_ingestion, PlannedSourceAsset, SourceIngestionPlan, SourcePlanError,
    SourcePlanMetadata,
};
