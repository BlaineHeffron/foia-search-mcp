use crate::ingest::{
    CancellationCheckpoint, CancellationSignal, CancellationToken, ChunkOptions, ExecutorError,
    ExtractedText, PageText, QueuedIngestionExecutor, SourcePlanError, TextExtraction,
    TextExtractor,
};
use crate::sources::{
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
    SourceFuture, SourceMetadata, SourceRecord, SourceStatus,
};
use crate::store::{ContentAddressedStore, NewIngestionJob, SqliteStore};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

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

    fn search<'a>(&'a self, _: &'a str, _: SearchOptions) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move { unreachable!("executor cancellation tests do not search") })
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

struct FixturePdfExtractor;

impl TextExtractor for FixturePdfExtractor {
    fn extract_pages(&self, _path: &std::path::Path) -> Result<ExtractedText, TextExtraction> {
        Ok(ExtractedText {
            pages: vec![
                PageText {
                    page_number: 1,
                    text: "alpha beta gamma".to_owned(),
                },
                PageText {
                    page_number: 2,
                    text: "delta epsilon zeta".to_owned(),
                },
            ],
            warnings: vec!["fixture warning".to_owned()],
        })
    }
}

struct CancelledPdfExtractor;

impl TextExtractor for CancelledPdfExtractor {
    fn extract_pages(&self, _path: &std::path::Path) -> Result<ExtractedText, TextExtraction> {
        Err(TextExtraction::Cancelled {
            binary: "fixture-pdftotext".into(),
            stderr: "cancelled".to_owned(),
        })
    }
}

struct StepCancellation {
    cancel_at: usize,
    probes: AtomicUsize,
}

impl StepCancellation {
    fn at(cancel_at: usize) -> Self {
        Self {
            cancel_at,
            probes: AtomicUsize::new(0),
        }
    }
}

impl CancellationSignal for StepCancellation {
    fn is_cancelled(&self) -> bool {
        self.probes.fetch_add(1, Ordering::Relaxed) + 1 >= self.cancel_at
    }
}

#[tokio::test]
async fn cancellation_after_claim_marks_job_interrupted() {
    let asset_url = fixture_http_url(b"%PDF cancel after claim");
    let mut store = SqliteStore::open_memory().expect("open store");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue(&mut store);
    let executor = fixture_executor(asset_url);
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = executor
        .run_next_with_ocr_and_cancel(
            &mut store,
            &files,
            &FixturePdfExtractor,
            &FixturePdfExtractor,
            &cancellation,
        )
        .await
        .expect_err("cancellation should interrupt claimed job");
    assert!(matches!(
        error,
        ExecutorError::Cancelled {
            checkpoint: CancellationCheckpoint::AfterClaim
        }
    ));
    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-cancel")
        .expect("job");
    assert_eq!(job.status, "interrupted");
    assert_eq!(job.stage, "queued");
    assert_eq!(job.progress, 0.0);
    assert_eq!(job.attempts, 1);
}

#[tokio::test]
async fn cancellation_before_persistence_interrupts_without_partial_rows() {
    let asset_url = fixture_http_url(b"%PDF cancel before persistence");
    let mut store = SqliteStore::open_memory().expect("open store");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue(&mut store);
    let executor =
        fixture_executor(asset_url).with_chunk_options(ChunkOptions { target_tokens: 3 });
    let cancellation = StepCancellation::at(7);

    let error = executor
        .run_next_with_ocr_and_cancel(
            &mut store,
            &files,
            &FixturePdfExtractor,
            &FixturePdfExtractor,
            &cancellation,
        )
        .await
        .expect_err("checkpoint cancellation should interrupt before persistence");
    assert!(matches!(
        error,
        ExecutorError::Cancelled {
            checkpoint: CancellationCheckpoint::BeforePersistence
        }
    ));
    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-cancel")
        .expect("job");
    assert_eq!(job.status, "interrupted");
    assert_eq!(job.stage, "persisting_document");
    assert_eq!(job.progress, 0.80);
    assert_eq!(row_count(&store, "documents"), 0);
    assert_eq!(row_count(&store, "assets"), 0);
    assert_eq!(row_count(&store, "pages"), 0);
    assert_eq!(row_count(&store, "chunks"), 0);
}

#[tokio::test]
async fn cancellation_after_source_resolution_is_resumable() {
    let asset_url = fixture_http_url(b"%PDF cancel after source resolution");
    let mut store = SqliteStore::open_memory().expect("open store");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue(&mut store);
    let executor =
        fixture_executor(asset_url).with_chunk_options(ChunkOptions { target_tokens: 3 });
    let cancellation = StepCancellation::at(2);

    let error = executor
        .run_next_with_ocr_and_cancel(
            &mut store,
            &files,
            &FixturePdfExtractor,
            &FixturePdfExtractor,
            &cancellation,
        )
        .await
        .expect_err("checkpoint cancellation should interrupt after source resolution");
    assert!(matches!(
        error,
        ExecutorError::Cancelled {
            checkpoint: CancellationCheckpoint::AfterSourceResolution
        }
    ));

    let interrupted = store
        .get_ingestion_job_record("ingest:cia:CREST-cancel")
        .expect("interrupted job");
    assert_eq!(interrupted.status, "interrupted");
    assert_eq!(interrupted.stage, "resolving_source_record");
    assert_eq!(interrupted.progress, 0.10);
    assert_eq!(interrupted.attempts, 1);
    assert_eq!(row_count(&store, "documents"), 0);
    assert_eq!(row_count(&store, "pages"), 0);
    assert_eq!(row_count(&store, "chunks"), 0);

    let resumed = executor
        .run_next(&mut store, &files, &FixturePdfExtractor)
        .await
        .expect("resume should succeed")
        .expect("interrupted job should be reclaimed");
    assert_eq!(resumed.page_count, 2);
    assert_eq!(resumed.chunk_count, 2);

    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-cancel")
        .expect("resumed job");
    assert_eq!(job.status, "succeeded");
    assert_eq!(job.attempts, 2);
    assert_eq!(row_count(&store, "documents"), 1);
    assert_eq!(row_count(&store, "pages"), 2);
    assert_eq!(row_count(&store, "chunks"), 2);
}

#[tokio::test]
async fn planning_failure_advances_to_planning_progress() {
    let mut record = source_record("https://example.test/page.jpg".to_owned());
    record.pdf_url = None;
    record.attachments = vec![SourceAsset {
        asset_url: "https://example.test/page.jpg".to_owned(),
        label: "Page image".to_owned(),
        mime_type: Some("image/jpeg".to_owned()),
        role: SourceAssetRole::Image,
    }];
    let mut store = SqliteStore::open_memory().expect("open store");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue(&mut store);
    let executor =
        QueuedIngestionExecutor::new("planning-worker", vec![Arc::new(FakeAdapter { record })])
            .expect("executor");

    let error = executor
        .run_next(&mut store, &files, &FixturePdfExtractor)
        .await
        .expect_err("non-ingestible assets should fail planning");

    assert!(matches!(
        error,
        ExecutorError::Plan(SourcePlanError::NoIngestibleAsset { .. })
    ));
    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-cancel")
        .expect("failed job");
    assert_eq!(job.status, "failed");
    assert_eq!(job.stage, "failed");
    assert_eq!(job.progress, 0.20);
    assert!(job
        .error
        .as_deref()
        .is_some_and(|error| error.contains("no ingestible PDF")));
    assert_eq!(row_count(&store, "documents"), 0);
    assert_eq!(row_count(&store, "assets"), 0);
    assert_eq!(row_count(&store, "pages"), 0);
    assert_eq!(row_count(&store, "chunks"), 0);
}

#[tokio::test]
async fn extraction_cancelled_error_marks_job_interrupted_not_failed() {
    let asset_url = fixture_http_url(b"%PDF cancelled extraction");
    let mut store = SqliteStore::open_memory().expect("open store");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue(&mut store);
    let executor = fixture_executor(asset_url);

    let error = executor
        .run_next_with_ocr(
            &mut store,
            &files,
            &CancelledPdfExtractor,
            &FixturePdfExtractor,
        )
        .await
        .expect_err("cancelled extraction should interrupt job");

    assert!(matches!(
        error,
        ExecutorError::Cancelled {
            checkpoint: CancellationCheckpoint::DuringExtraction
        }
    ));
    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-cancel")
        .expect("job");
    assert_eq!(job.status, "interrupted");
    assert_eq!(job.stage, "extracting_text");
    assert_eq!(job.progress, 0.60);
    assert_eq!(row_count(&store, "documents"), 0);
}

#[tokio::test]
async fn cancellation_before_asset_provenance_write_keeps_document_rows_resumable() {
    let asset_url = fixture_http_url(b"%PDF cancel before asset write");
    let mut store = SqliteStore::open_memory().expect("open store");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue(&mut store);
    let executor =
        fixture_executor(asset_url).with_chunk_options(ChunkOptions { target_tokens: 3 });
    let cancellation = StepCancellation::at(8);

    let error = executor
        .run_next_with_ocr_and_cancel(
            &mut store,
            &files,
            &FixturePdfExtractor,
            &FixturePdfExtractor,
            &cancellation,
        )
        .await
        .expect_err("checkpoint cancellation should interrupt before asset write");
    assert!(matches!(
        error,
        ExecutorError::Cancelled {
            checkpoint: CancellationCheckpoint::BeforeAssetProvenanceWrite
        }
    ));
    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-cancel")
        .expect("job");
    assert_eq!(job.status, "interrupted");
    assert_eq!(job.stage, "persisting_asset");
    assert_eq!(job.progress, 0.90);
    assert_eq!(row_count(&store, "documents"), 1);
    assert_eq!(row_count(&store, "pages"), 2);
    assert_eq!(row_count(&store, "chunks"), 2);
    assert_eq!(row_count(&store, "assets"), 0);
}

fn fixture_executor(asset_url: String) -> QueuedIngestionExecutor {
    QueuedIngestionExecutor::new(
        "cancel-worker",
        vec![Arc::new(FakeAdapter {
            record: source_record(asset_url),
        })],
    )
    .expect("executor")
}

fn enqueue(store: &mut SqliteStore) {
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: "ingest:cia:CREST-cancel".to_owned(),
            operation: "ingest".to_owned(),
            source: "cia".to_owned(),
            source_id: Some("CREST-cancel".to_owned()),
            target_url: None,
            next_action: "queued".to_owned(),
        })
        .expect("create job");
}

fn source_record(asset_url: String) -> SourceRecord {
    SourceRecord {
        id: "cia:CREST-cancel".to_owned(),
        document_key: "cia_CREST-cancel".to_owned(),
        source: "cia",
        source_id: "CREST-cancel".to_owned(),
        title: "Cancel Fixture".to_owned(),
        date: None,
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("executor cancellation test".to_owned()),
        origin_url: "https://www.cia.gov/readingroom/document/CREST-cancel".to_owned(),
        document_url: "https://www.cia.gov/readingroom/document/CREST-cancel".to_owned(),
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

fn row_count(store: &SqliteStore, table: &str) -> i64 {
    store
        .connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("row count")
}

fn fixture_http_url(body: &'static [u8]) -> String {
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
