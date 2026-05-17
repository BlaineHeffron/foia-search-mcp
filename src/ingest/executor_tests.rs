use crate::ingest::{
    ChunkOptions, ExtractedText, IngestionJobLease, PageText, QueuedIngestionExecutor,
    TextExtraction, TextExtractor,
};
use crate::sources::{
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
    SourceFuture, SourceMetadata, SourceRecord, SourceStatus,
};
use crate::store::{ContentAddressedStore, NewIngestionJob, SqliteStore};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

#[derive(Clone)]
struct FakeAdapter {
    record: SourceRecord,
    cache_policy: CachePolicy,
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
        self.cache_policy.clone()
    }
}

struct FakePdfExtractor;

impl TextExtractor for FakePdfExtractor {
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
            warnings: vec!["embedded PDF text has fixture warning".to_owned()],
        })
    }
}

fn source_record(asset_url: String) -> SourceRecord {
    SourceRecord {
        id: "cia:CREST-executor".to_owned(),
        document_key: "cia_CREST-executor".to_owned(),
        source: "cia",
        source_id: "CREST-executor".to_owned(),
        title: "Executor Fixture".to_owned(),
        date: None,
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("executor test".to_owned()),
        origin_url: "https://www.cia.gov/readingroom/document/CREST-executor".to_owned(),
        document_url: "https://www.cia.gov/readingroom/document/CREST-executor".to_owned(),
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

fn enqueue(store: &mut SqliteStore) {
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: "ingest:cia:CREST-executor".to_owned(),
            operation: "ingest".to_owned(),
            source: "cia".to_owned(),
            source_id: Some("CREST-executor".to_owned()),
            target_url: None,
            next_action: "queued".to_owned(),
        })
        .expect("create job");
}

#[tokio::test]
async fn run_next_ingests_document_asset_pages_chunks_and_warnings() {
    let asset_url = fixture_http_url(b"%PDF fixture body");
    let mut store = SqliteStore::open_memory().expect("open store");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue(&mut store);

    let executor = QueuedIngestionExecutor::new(
        "test-worker",
        vec![Arc::new(FakeAdapter {
            record: source_record(asset_url),
            cache_policy: CachePolicy::RespectSourceHeaders,
        })],
    )
    .expect("executor")
    .with_chunk_options(ChunkOptions { target_tokens: 3 });

    let outcome = executor
        .run_next(&mut store, &files, &FakePdfExtractor)
        .await
        .expect("executor should run")
        .expect("job should be claimed");

    assert_eq!(outcome.document_key, "cia_CREST-executor");
    assert_eq!(outcome.page_count, 2);
    assert_eq!(outcome.chunk_count, 2);

    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-executor")
        .expect("job");
    assert_eq!(job.status, "succeeded");
    assert_eq!(job.stage, "succeeded");
    assert_eq!(job.progress, 1.0);
    assert!(job.document_id.is_some());
    assert_eq!(
        job.warnings,
        vec!["embedded PDF text has fixture warning".to_owned()]
    );

    let metadata = store
        .get_document_metadata("cia:CREST-executor")
        .expect("metadata");
    assert_eq!(metadata.page_count, 2);
    let asset_count: i64 = store
        .connection()
        .query_row("SELECT count(*) FROM assets", [], |row| row.get(0))
        .expect("asset count");
    assert_eq!(asset_count, 1);
}

#[tokio::test]
async fn failed_download_marks_job_failed_and_can_claim_requeued_job() {
    let mut store = SqliteStore::open_memory().expect("open store");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue(&mut store);

    let executor = QueuedIngestionExecutor::new(
        "test-worker",
        vec![Arc::new(FakeAdapter {
            record: source_record(fixture_http_status(500)),
            cache_policy: CachePolicy::RespectSourceHeaders,
        })],
    )
    .expect("executor");

    let error = executor
        .run_next(&mut store, &files, &FakePdfExtractor)
        .await
        .expect_err("download should fail");
    assert!(error.to_string().contains("HTTP 500"));
    let failed = store
        .get_ingestion_job_record("ingest:cia:CREST-executor")
        .expect("failed job");
    assert_eq!(failed.status, "failed");
    assert!(failed.error.is_some());
    assert_eq!(row_count(&store, "documents"), 0);
    assert_eq!(row_count(&store, "assets"), 0);
    assert_eq!(row_count(&store, "pages"), 0);
    assert_eq!(row_count(&store, "chunks"), 0);

    store
        .connection()
        .execute(
            "UPDATE ingestion_jobs SET status = 'interrupted', lease_expires_at = '1970-01-01T00:00:00.000Z' WHERE job_key = ?1",
            ["ingest:cia:CREST-executor"],
        )
        .expect("requeue as interrupted");
    let claim = store
        .claim_next_ingestion_job(&IngestionJobLease {
            owner: "resume-worker".to_owned(),
            now: "2026-01-01T00:00:00.000Z".to_owned(),
            expires_at: "2026-01-01T00:05:00.000Z".to_owned(),
        })
        .expect("claim interrupted");
    assert!(claim.is_some());
}

#[test]
fn stale_worker_cannot_record_warning_after_reclaim() {
    let mut store = SqliteStore::open_memory().expect("open store");
    enqueue(&mut store);

    let first = store
        .claim_next_ingestion_job(&IngestionJobLease {
            owner: "stale-worker".to_owned(),
            now: "2026-01-01T00:00:00.000Z".to_owned(),
            expires_at: "2026-01-01T00:05:00.000Z".to_owned(),
        })
        .expect("claim first");
    assert!(first.is_some());
    store
        .connection()
        .execute(
            "UPDATE ingestion_jobs SET lease_expires_at = '2026-01-01T00:00:00.000Z' WHERE job_key = ?1",
            ["ingest:cia:CREST-executor"],
        )
        .expect("expire first lease");
    let reclaimed = store
        .claim_next_ingestion_job(&IngestionJobLease {
            owner: "fresh-worker".to_owned(),
            now: "2026-01-01T00:00:01.000Z".to_owned(),
            expires_at: "2026-01-01T00:05:01.000Z".to_owned(),
        })
        .expect("claim expired job");
    assert!(reclaimed.is_some());

    let stale_result = store.record_ingestion_job_warning(
        "ingest:cia:CREST-executor",
        "stale-worker",
        "stale warning",
    );
    assert!(stale_result.is_err());
    let job = store
        .get_ingestion_job_record("ingest:cia:CREST-executor")
        .expect("job");
    assert_eq!(job.lease_owner.as_deref(), Some("fresh-worker"));
    assert!(job.warnings.is_empty());
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

fn fixture_http_status(status: u16) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture addr");
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 {status} Fixture Error\r\nContent-Length: 0\r\n\r\n",
            )
            .expect("write error response");
        }
    });
    format!("http://{addr}/fixture.pdf")
}

fn read_http_request(stream: &mut TcpStream) {
    let mut buf = [0_u8; 1024];
    let _ = stream.read(&mut buf);
}
