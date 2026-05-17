use crate::ingest::ocr::OCR_FALLBACK_INCOMPATIBLE_WARNING;
use crate::ingest::{
    select_pdf_text, ChunkOptions, ExtractedText, OcrFallbackPolicy, PageText,
    QueuedIngestionExecutor, TextExtraction, TextExtractor, TextFileExtractor,
};
use crate::sources::{
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
    SourceFuture, SourceMetadata, SourceRecord, SourceStatus,
};
use crate::store::{ContentAddressedStore, NewIngestionJob, SqliteStore, TextSource};
use std::cell::Cell;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;

struct StaticExtractor {
    extracted: ExtractedText,
    calls: Cell<usize>,
}

impl StaticExtractor {
    fn new(extracted: ExtractedText) -> Self {
        Self {
            extracted,
            calls: Cell::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.get()
    }
}

impl TextExtractor for StaticExtractor {
    fn extract_pages(&self, _path: &Path) -> Result<ExtractedText, TextExtraction> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.extracted.clone())
    }
}

struct FailingExtractor {
    calls: Cell<usize>,
}

impl FailingExtractor {
    fn new() -> Self {
        Self {
            calls: Cell::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.get()
    }
}

impl TextExtractor for FailingExtractor {
    fn extract_pages(&self, _path: &Path) -> Result<ExtractedText, TextExtraction> {
        self.calls.set(self.calls.get() + 1);
        Err(TextExtraction::EmptyInput)
    }
}

#[test]
fn embedded_pdf_text_without_warnings_does_not_try_ocr() {
    let embedded = StaticExtractor::new(extracted(&["clear embedded text"], Vec::new()));
    let ocr = StaticExtractor::new(extracted(&["ocr text"], Vec::new()));

    let selected = select_pdf_text(
        Path::new("fixture.pdf"),
        &embedded,
        &ocr,
        OcrFallbackPolicy::on_quality_warning(),
    )
    .expect("select embedded text");

    assert_eq!(selected.text_source, TextSource::EmbeddedPdfText);
    assert_eq!(selected.extracted.pages[0].text, "clear embedded text");
    assert_eq!(ocr.calls(), 0);
}

#[test]
fn embedded_pdf_text_with_warnings_keeps_embedded_when_ocr_disabled() {
    let embedded = StaticExtractor::new(extracted(&["thin"], vec!["low density"]));
    let ocr = StaticExtractor::new(extracted(&["ocr text"], Vec::new()));

    let selected = select_pdf_text(
        Path::new("fixture.pdf"),
        &embedded,
        &ocr,
        OcrFallbackPolicy::off(),
    )
    .expect("select embedded text");

    assert_eq!(selected.text_source, TextSource::EmbeddedPdfText);
    assert_eq!(selected.extracted.warnings, vec!["low density"]);
    assert_eq!(ocr.calls(), 0);
}

#[test]
fn ocr_policy_parses_only_explicit_opt_in_as_enabled() {
    assert!(!OcrFallbackPolicy::from_env_value(None).is_enabled());
    assert!(!OcrFallbackPolicy::from_env_value(Some("")).is_enabled());
    assert!(!OcrFallbackPolicy::from_env_value(Some("on")).is_enabled());
    assert!(!OcrFallbackPolicy::from_env_value(Some("off")).is_enabled());
    assert!(OcrFallbackPolicy::from_env_value(Some("on_quality_warning")).is_enabled());
}

#[test]
fn embedded_pdf_text_with_warnings_uses_compatible_ocr_when_enabled() {
    let embedded = StaticExtractor::new(extracted(&["thin"], vec!["low density"]));
    let ocr = StaticExtractor::new(extracted(&["ocr replacement"], Vec::new()));

    let selected = select_pdf_text(
        Path::new("fixture.pdf"),
        &embedded,
        &ocr,
        OcrFallbackPolicy::on_quality_warning(),
    )
    .expect("select OCR text");

    assert_eq!(selected.text_source, TextSource::LocalOcr);
    assert_eq!(selected.extracted.pages[0].text, "ocr replacement");
    assert_eq!(
        selected.extracted.warnings,
        vec![
            "low density",
            "local OCR fallback was used after embedded PDF text quality warnings"
        ]
    );
}

#[test]
fn incompatible_ocr_page_boundaries_keep_embedded_pdf_text() {
    let embedded = StaticExtractor::new(extracted(&["thin one", "thin two"], vec!["low density"]));
    let mut ocr_text = extracted(&["ocr one"], Vec::new());
    ocr_text.pages[0].page_number = 2;
    let ocr = StaticExtractor::new(ocr_text);

    let selected = select_pdf_text(
        Path::new("fixture.pdf"),
        &embedded,
        &ocr,
        OcrFallbackPolicy::on_quality_warning(),
    )
    .expect("select embedded text after incompatible OCR");

    assert_eq!(selected.text_source, TextSource::EmbeddedPdfText);
    assert_eq!(selected.extracted.pages[0].text, "thin one");
    assert_eq!(selected.extracted.pages[1].text, "thin two");
    assert_eq!(
        selected.extracted.warnings,
        vec!["low density", OCR_FALLBACK_INCOMPATIBLE_WARNING]
    );
    assert_eq!(ocr.calls(), 1);
}

#[test]
fn ocr_failure_falls_back_to_embedded_pdf_text_and_warnings() {
    let embedded = StaticExtractor::new(extracted(&["thin"], vec!["low density"]));
    let ocr = FailingExtractor::new();

    let selected = select_pdf_text(
        Path::new("fixture.pdf"),
        &embedded,
        &ocr,
        OcrFallbackPolicy::on_quality_warning(),
    )
    .expect("select embedded fallback");

    assert_eq!(selected.text_source, TextSource::EmbeddedPdfText);
    assert_eq!(selected.extracted.pages[0].text, "thin");
    assert_eq!(selected.extracted.warnings, vec!["low density"]);
    assert_eq!(ocr.calls(), 1);
}

#[test]
fn embedded_failure_is_rescued_by_ocr_when_enabled() {
    let embedded = FailingExtractor::new();
    let ocr = StaticExtractor::new(extracted(&["ocr rescue"], Vec::new()));

    let selected = select_pdf_text(
        Path::new("fixture.pdf"),
        &embedded,
        &ocr,
        OcrFallbackPolicy::on_quality_warning(),
    )
    .expect("select OCR rescue");

    assert_eq!(selected.text_source, TextSource::LocalOcr);
    assert_eq!(selected.extracted.pages[0].text, "ocr rescue");
    assert_eq!(
        selected.extracted.warnings,
        vec!["local OCR fallback was used after embedded PDF text extraction failed"]
    );
}

#[tokio::test]
async fn non_pdf_text_asset_bypasses_ocr_selector() {
    let asset_url = fixture_http_url(b"source OCR page");
    let mut store = SqliteStore::open_memory().expect("open store");
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: "ingest:cia:CREST-ocr-text".to_owned(),
            operation: "ingest".to_owned(),
            source: "cia".to_owned(),
            source_id: Some("CREST-ocr-text".to_owned()),
            target_url: None,
            next_action: "queued".to_owned(),
        })
        .expect("create job");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    let ocr = StaticExtractor::new(extracted(&["should not be used"], Vec::new()));
    let executor = QueuedIngestionExecutor::new(
        "ocr-bypass-worker",
        vec![Arc::new(FakeAdapter {
            record: source_ocr_record(asset_url),
        })],
    )
    .expect("executor")
    .with_chunk_options(ChunkOptions { target_tokens: 10 })
    .with_ocr_policy(OcrFallbackPolicy::on_quality_warning());

    let outcome = executor
        .run_next_with_ocr(&mut store, &files, &TextFileExtractor, &ocr)
        .await
        .expect("run executor")
        .expect("claimed job");

    assert_eq!(outcome.page_count, 1);
    assert_eq!(ocr.calls(), 0);
    let pages = store
        .get_page_text("cia:CREST-ocr-text", 1, 1)
        .expect("stored page text");
    assert_eq!(pages[0].text_source, "source_ocr");
    assert_eq!(pages[0].text, "source OCR page");
}

#[tokio::test]
async fn incompatible_ocr_page_boundaries_record_durable_job_warning() {
    let asset_url = fixture_pdf_http_url(b"%PDF mismatch body");
    let mut store = SqliteStore::open_memory().expect("open store");
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: "ingest:cia:CREST-ocr-pdf".to_owned(),
            operation: "ingest".to_owned(),
            source: "cia".to_owned(),
            source_id: Some("CREST-ocr-pdf".to_owned()),
            target_url: None,
            next_action: "queued".to_owned(),
        })
        .expect("create job");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    let embedded = StaticExtractor::new(extracted(&["thin one", "thin two"], vec!["low density"]));
    let mut ocr_extracted = extracted(&["ocr mismatch"], vec!["ocr warning"]);
    ocr_extracted.pages[0].page_number = 99;
    let ocr = StaticExtractor::new(ocr_extracted);
    let executor = QueuedIngestionExecutor::new(
        "ocr-mismatch-worker",
        vec![Arc::new(FakeAdapter {
            record: source_pdf_record(asset_url),
        })],
    )
    .expect("executor")
    .with_chunk_options(ChunkOptions { target_tokens: 10 })
    .with_ocr_policy(OcrFallbackPolicy::on_quality_warning());

    let outcome = executor
        .run_next_with_ocr(&mut store, &files, &embedded, &ocr)
        .await
        .expect("run executor")
        .expect("claimed job");

    assert_eq!(
        outcome.warnings,
        vec![
            "low density".to_owned(),
            OCR_FALLBACK_INCOMPATIBLE_WARNING.to_owned()
        ]
    );
    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-ocr-pdf")
        .expect("job");
    assert_eq!(
        job.warnings,
        vec![
            "low density".to_owned(),
            OCR_FALLBACK_INCOMPATIBLE_WARNING.to_owned()
        ]
    );
    let pages = store
        .get_page_text("cia:CREST-ocr-pdf", 1, 2)
        .expect("stored page text");
    assert_eq!(pages[0].text, "thin one");
    assert_eq!(pages[1].text, "thin two");
    assert_eq!(ocr.calls(), 1);
}

fn extracted(pages: &[&str], warnings: Vec<&str>) -> ExtractedText {
    ExtractedText {
        pages: pages
            .iter()
            .enumerate()
            .map(|(index, text)| PageText {
                page_number: (index + 1) as u32,
                text: (*text).to_owned(),
            })
            .collect(),
        warnings: warnings.into_iter().map(str::to_owned).collect(),
    }
}

#[derive(Clone)]
struct FakeAdapter {
    record: SourceRecord,
}

impl SourceAdapter for FakeAdapter {
    fn name(&self) -> &'static str {
        "cia"
    }

    fn status(&self) -> SourceStatus {
        SourceStatus::Enabled
    }

    fn search<'a>(
        &'a self,
        _query: &'a str,
        _options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move {
            Ok(SearchPage {
                query: String::new(),
                source: "cia",
                records: vec![self.record.clone()],
                next_cursor: None,
                warnings: Vec::new(),
            })
        })
    }

    fn get_record<'a>(&'a self, _id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move { Ok(self.record.clone()) })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(record.attachments.clone()) })
    }

    fn cache_policy(&self) -> CachePolicy {
        CachePolicy::RespectSourceHeaders
    }
}

fn source_ocr_record(asset_url: String) -> SourceRecord {
    SourceRecord {
        id: "cia:CREST-ocr-text".to_owned(),
        document_key: "cia_CREST-ocr-text".to_owned(),
        source: "cia",
        source_id: "CREST-ocr-text".to_owned(),
        title: "OCR Text Fixture".to_owned(),
        date: None,
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("ocr text executor test".to_owned()),
        origin_url: "https://www.cia.gov/readingroom/document/CREST-ocr-text".to_owned(),
        document_url: "https://www.cia.gov/readingroom/document/CREST-ocr-text".to_owned(),
        pdf_url: None,
        metadata: SourceMetadata::new(),
        attachments: vec![SourceAsset {
            asset_url,
            label: "OCR text".to_owned(),
            mime_type: Some("text/plain".to_owned()),
            role: SourceAssetRole::OcrText,
        }],
        text_preview: None,
        citation_note: Some("cite source".to_owned()),
        terms_note: Some("terms".to_owned()),
    }
}

fn source_pdf_record(asset_url: String) -> SourceRecord {
    SourceRecord {
        id: "cia:CREST-ocr-pdf".to_owned(),
        document_key: "cia_CREST-ocr-pdf".to_owned(),
        source: "cia",
        source_id: "CREST-ocr-pdf".to_owned(),
        title: "OCR PDF Fixture".to_owned(),
        date: None,
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("ocr mismatch executor test".to_owned()),
        origin_url: "https://www.cia.gov/readingroom/document/CREST-ocr-pdf".to_owned(),
        document_url: "https://www.cia.gov/readingroom/document/CREST-ocr-pdf".to_owned(),
        pdf_url: Some(asset_url.clone()),
        metadata: SourceMetadata::new(),
        attachments: vec![SourceAsset {
            asset_url,
            label: "PDF".to_owned(),
            mime_type: Some("application/pdf".to_owned()),
            role: SourceAssetRole::Pdf,
        }],
        text_preview: None,
        citation_note: Some("cite source".to_owned()),
        terms_note: Some("terms".to_owned()),
    }
}

fn fixture_http_url(body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture addr");
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .expect("write response headers");
            stream.write_all(body).expect("write response body");
        }
    });
    format!("http://{addr}/fixture.txt")
}

fn fixture_pdf_http_url(body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture addr");
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .expect("write response headers");
            stream.write_all(body).expect("write response body");
        }
    });
    format!("http://{addr}/fixture.pdf")
}

fn read_http_request(stream: &mut TcpStream) {
    let mut buf = [0_u8; 1024];
    let _ = stream.read(&mut buf);
}
