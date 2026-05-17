use crate::ingest::{
    chunk::{chunk_pages, ChunkError, ChunkOptions},
    pdf::{ExtractedText, TextExtraction, TextExtractor, TextFileExtractor},
};
use crate::store::{
    ChunkInput, DocumentKey, PageInput, SqliteStore, StoreError, TextSource, UpsertDocument,
};
use std::fmt;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct IngestDocument {
    pub public_id: String,
    pub document_key: DocumentKey,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub date: Option<String>,
    pub collection: Option<String>,
    pub record_group: Option<String>,
    pub description: Option<String>,
    pub origin_url: Option<String>,
    pub document_url: Option<String>,
    pub pdf_url: Option<String>,
    pub metadata_json: String,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
    pub text_source: TextSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestOutcome {
    pub document_key: DocumentKey,
    pub page_count: usize,
    pub chunk_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum IngestError {
    Store(StoreError),
    Extraction(TextExtraction),
    Chunk(ChunkError),
    TokenEstimateOverflow { chunk_id: String, tokens: usize },
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(err) => write!(f, "{err}"),
            Self::Extraction(err) => write!(f, "{err}"),
            Self::Chunk(err) => write!(f, "{err}"),
            Self::TokenEstimateOverflow { chunk_id, tokens } => write!(
                f,
                "token estimate for chunk {chunk_id} does not fit in SQLite integer: {tokens}"
            ),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<StoreError> for IngestError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

impl From<TextExtraction> for IngestError {
    fn from(err: TextExtraction) -> Self {
        Self::Extraction(err)
    }
}

impl From<ChunkError> for IngestError {
    fn from(err: ChunkError) -> Self {
        Self::Chunk(err)
    }
}

pub fn ingest_text_file(
    store: &mut SqliteStore,
    path: &Path,
    document: IngestDocument,
    chunk_options: &ChunkOptions,
) -> Result<IngestOutcome, IngestError> {
    ingest_with_extractor(store, path, document, chunk_options, &TextFileExtractor)
}

pub fn ingest_with_extractor(
    store: &mut SqliteStore,
    path: &Path,
    document: IngestDocument,
    chunk_options: &ChunkOptions,
    extractor: &dyn TextExtractor,
) -> Result<IngestOutcome, IngestError> {
    let extracted = extractor.extract_pages(path)?;
    ingest_extracted_text(store, document, chunk_options, extracted, None)
}

pub fn ingest_extracted_text(
    store: &mut SqliteStore,
    document: IngestDocument,
    chunk_options: &ChunkOptions,
    extracted: ExtractedText,
    text_source: Option<TextSource>,
) -> Result<IngestOutcome, IngestError> {
    let text_source = text_source.unwrap_or(document.text_source);
    let chunks = chunk_pages(&extracted.pages, chunk_options)?;
    let pages = extracted
        .pages
        .iter()
        .map(|page| PageInput {
            document_key: document.document_key.clone(),
            page_number: i64::from(page.page_number),
            text: page.text.clone(),
            text_source,
            quality_score: None,
            warnings_json: "[]".to_owned(),
        })
        .collect::<Vec<_>>();
    let chunk_inputs = chunks
        .iter()
        .map(|chunk| {
            let token_estimate = i64::try_from(chunk.token_estimate).map_err(|_| {
                IngestError::TokenEstimateOverflow {
                    chunk_id: chunk.chunk_id.clone(),
                    tokens: chunk.token_estimate,
                }
            })?;
            Ok(ChunkInput {
                document_key: document.document_key.clone(),
                chunk_id: chunk.chunk_id.clone(),
                page_start: i64::from(chunk.page_start),
                page_end: i64::from(chunk.page_end),
                text: chunk.text.clone(),
                token_estimate: Some(token_estimate),
                metadata_json: "{}".to_owned(),
            })
        })
        .collect::<Result<Vec<_>, IngestError>>()?;

    store.upsert_document(&UpsertDocument {
        public_id: document.public_id,
        document_key: document.document_key.clone(),
        source: document.source,
        source_id: document.source_id,
        title: document.title,
        date: document.date,
        collection: document.collection,
        record_group: document.record_group,
        description: document.description,
        origin_url: document.origin_url,
        document_url: document.document_url,
        pdf_url: document.pdf_url,
        metadata_json: document.metadata_json,
        citation_note: document.citation_note,
        terms_note: document.terms_note,
    })?;

    store.replace_pages_and_chunks(&document.document_key, &pages, &chunk_inputs)?;

    Ok(IngestOutcome {
        document_key: document.document_key,
        page_count: pages.len(),
        chunk_count: chunk_inputs.len(),
        warnings: extracted.warnings,
    })
}
