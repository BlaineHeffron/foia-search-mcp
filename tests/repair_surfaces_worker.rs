use foia_search::{
    index::reconcile_sqlite_fts_index,
    ingest::{reconcile::reconcile_derived_artifacts_for_document, QueuedIngestionWorker},
    sources::{
        CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
        SourceFuture, SourceMetadata, SourceRecord, SourceStatus,
    },
    store::{
        ChunkInput, ContentAddressedStore, DocumentKey, NewIngestionJob, PageInput, SqliteStore,
        TextSource, UpsertDocument,
    },
};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const SENTINEL_DOCUMENT_ID: &str = "cia:CREST-repair-sentinel";
const SENTINEL_DOCUMENT_KEY: &str = "cia_CREST-repair-sentinel";
const SENTINEL_STALE_DOCUMENT_TEXT: &str = "stale derived document text";
const SENTINEL_STALE_PAGE_TEXT: &str = "stale derived page text";
const SENTINEL_STALE_FTS_BODY: &str = "stale repair sentinel fts body";

const WORKER_JOB_KEY: &str = "ingest:fixture:worker-repair-boundary";
const WORKER_DOCUMENT_ID: &str = "fixture:worker-repair-boundary";
const WORKER_DOCUMENT_KEY: &str = "fixture_worker_repair_boundary";

#[test]
fn queued_worker_reclaims_interrupted_job_without_invoking_repair_surfaces() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let data_dir = tempdir.path().join("data");
    let files = ContentAddressedStore::new(&data_dir);
    let mut store = open_store(&data_dir);

    seed_repair_drift(&mut store, &files);
    let before = DriftSnapshot::capture(&store, &files);

    let server = SingleResponseFixtureServer::start(worker_text_body());
    enqueue_interrupted_worker_job(&mut store);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create test runtime");
    let worker = QueuedIngestionWorker::new(
        data_dir.clone(),
        vec![Arc::new(FakeAdapter {
            record: worker_source_record(server.asset_url.clone()),
        })],
    );

    let outcome = runtime
        .block_on(worker.run_once())
        .expect("worker run should succeed")
        .expect("interrupted job should be reclaimed");
    assert_eq!(outcome.job_key, WORKER_JOB_KEY);
    assert_eq!(outcome.document_key.as_str(), WORKER_DOCUMENT_KEY);

    let finished = store
        .get_ingestion_job_record(WORKER_JOB_KEY)
        .expect("load completed worker job");
    assert_eq!(finished.status, "succeeded");
    assert_eq!(finished.attempts, 2);
    assert_eq!(worker_row_count(&store), 1);

    let after = DriftSnapshot::capture(&store, &files);
    assert_eq!(before, after);
    assert_eq!(
        fs::read_to_string(files.derived_document_text_path(&sentinel_key()))
            .expect("read sentinel document artifact"),
        SENTINEL_STALE_DOCUMENT_TEXT
    );
    assert_eq!(
        fs::read_to_string(files.derived_page_text_path(&sentinel_key(), 1))
            .expect("read sentinel page artifact"),
        SENTINEL_STALE_PAGE_TEXT
    );
    assert_eq!(fts_body(&store), SENTINEL_STALE_FTS_BODY);
}

#[derive(Debug, Eq, PartialEq)]
struct DriftSnapshot {
    document_text: String,
    page_text: String,
    fts_body: String,
    derived_issue_count: usize,
    fts_issue_count: usize,
}

impl DriftSnapshot {
    fn capture(store: &SqliteStore, files: &ContentAddressedStore) -> Self {
        let key = sentinel_key();
        let derived_report =
            reconcile_derived_artifacts_for_document(store, files, SENTINEL_DOCUMENT_KEY)
                .expect("report sentinel derived drift");
        let fts_report = reconcile_sqlite_fts_index(store).expect("report sentinel fts drift");

        assert!(
            !derived_report.issues.is_empty(),
            "sentinel derived drift should be present before and after worker reclaim"
        );
        assert!(
            fts_report
                .issues
                .iter()
                .any(|issue| issue.document_key == SENTINEL_DOCUMENT_KEY),
            "sentinel fts drift should be present before and after worker reclaim"
        );

        Self {
            document_text: fs::read_to_string(files.derived_document_text_path(&key))
                .expect("read sentinel document artifact"),
            page_text: fs::read_to_string(files.derived_page_text_path(&key, 1))
                .expect("read sentinel page artifact"),
            fts_body: fts_body(store),
            derived_issue_count: derived_report.issues.len(),
            fts_issue_count: fts_report.issues.len(),
        }
    }
}

fn seed_repair_drift(store: &mut SqliteStore, files: &ContentAddressedStore) {
    let key = sentinel_key();
    store
        .upsert_document(&UpsertDocument {
            public_id: SENTINEL_DOCUMENT_ID.to_owned(),
            document_key: key.clone(),
            source: "cia".to_owned(),
            source_id: "CREST-repair-sentinel".to_owned(),
            title: "Repair Sentinel".to_owned(),
            date: Some("1962-08-01".to_owned()),
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("repair auto-invocation sentinel".to_owned()),
            origin_url: Some(
                "https://www.cia.gov/readingroom/document/CREST-repair-sentinel".to_owned(),
            ),
            document_url: Some(
                "https://www.cia.gov/readingroom/document/CREST-repair-sentinel".to_owned(),
            ),
            pdf_url: None,
            metadata_json: "{}".to_owned(),
            citation_note: None,
            terms_note: None,
        })
        .expect("seed sentinel document");
    store
        .replace_pages_and_chunks(
            &key,
            &[PageInput {
                document_key: key.clone(),
                page_number: 1,
                text: "canonical sentinel page text".to_owned(),
                text_source: TextSource::EmbeddedPdfText,
                quality_score: Some(0.95),
                warnings_json: "[]".to_owned(),
            }],
            &[ChunkInput {
                document_key: key.clone(),
                chunk_id: "chunk-1".to_owned(),
                page_start: 1,
                page_end: 1,
                text: "canonical sentinel chunk text".to_owned(),
                token_estimate: Some(4),
                metadata_json: "{}".to_owned(),
            }],
        )
        .expect("seed sentinel canonical rows");
    store
        .connection()
        .execute(
            "UPDATE chunk_fts SET body = ?1 WHERE document_key = ?2 AND chunk_id = 'chunk-1'",
            (SENTINEL_STALE_FTS_BODY, SENTINEL_DOCUMENT_KEY),
        )
        .expect("seed stale sentinel fts row");

    write_with_parent(
        &files.derived_document_text_path(&key),
        SENTINEL_STALE_DOCUMENT_TEXT,
    );
    write_with_parent(
        &files.derived_page_text_path(&key, 1),
        SENTINEL_STALE_PAGE_TEXT,
    );
}

fn enqueue_interrupted_worker_job(store: &mut SqliteStore) {
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: WORKER_JOB_KEY.to_owned(),
            operation: "ingest".to_owned(),
            source: "fixture".to_owned(),
            source_id: Some("worker-repair-boundary".to_owned()),
            target_url: None,
            next_action: "resume by reclaiming interrupted test job".to_owned(),
        })
        .expect("create worker job");
    store
        .connection()
        .execute(
            "
            UPDATE ingestion_jobs
            SET status = 'interrupted',
                stage = 'extracting_text',
                progress = 0.60,
                attempts = 1,
                lease_owner = NULL,
                lease_expires_at = NULL,
                error = 'seeded interrupted worker job'
            WHERE job_key = ?1
            ",
            [WORKER_JOB_KEY],
        )
        .expect("mark worker job interrupted");
}

fn open_store(data_dir: &Path) -> SqliteStore {
    let db_dir = data_dir.join("db");
    fs::create_dir_all(&db_dir).expect("create db dir");
    SqliteStore::open(db_dir.join("foia.sqlite")).expect("open sqlite store")
}

fn sentinel_key() -> DocumentKey {
    DocumentKey::new(SENTINEL_DOCUMENT_KEY).expect("sentinel document key")
}

fn write_with_parent(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create artifact parent");
    }
    fs::write(path, body).expect("write artifact fixture");
}

fn fts_body(store: &SqliteStore) -> String {
    store
        .connection()
        .query_row(
            "SELECT body FROM chunk_fts WHERE document_key = ?1 AND chunk_id = 'chunk-1'",
            [SENTINEL_DOCUMENT_KEY],
            |row| row.get(0),
        )
        .expect("load sentinel fts body")
}

fn worker_row_count(store: &SqliteStore) -> i64 {
    store
        .connection()
        .query_row(
            "SELECT count(*) FROM documents WHERE public_id = ?1",
            [WORKER_DOCUMENT_ID],
            |row| row.get(0),
        )
        .expect("count worker document rows")
}

#[derive(Clone)]
struct FakeAdapter {
    record: SourceRecord,
}

impl SourceAdapter for FakeAdapter {
    fn name(&self) -> &'static str {
        "fixture"
    }

    fn status(&self) -> SourceStatus {
        SourceStatus::Enabled
    }

    fn search<'a>(
        &'a self,
        _query: &'a str,
        _options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move { unreachable!("worker repair boundary test does not call search") })
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

fn worker_source_record(asset_url: String) -> SourceRecord {
    SourceRecord {
        id: WORKER_DOCUMENT_ID.to_owned(),
        document_key: WORKER_DOCUMENT_KEY.to_owned(),
        source: "fixture",
        source_id: "worker-repair-boundary".to_owned(),
        title: "Worker Repair Boundary".to_owned(),
        date: Some("1962-08-01".to_owned()),
        collection: Some("Fixture".to_owned()),
        record_group: None,
        description: Some("Worker should drain this without invoking repair surfaces".to_owned()),
        origin_url: "https://fixture.test/worker-repair-boundary".to_owned(),
        document_url: "https://fixture.test/worker-repair-boundary".to_owned(),
        pdf_url: None,
        metadata: SourceMetadata::new(),
        attachments: vec![SourceAsset {
            asset_url,
            label: "Plain text".to_owned(),
            mime_type: Some("text/plain".to_owned()),
            role: SourceAssetRole::Other,
        }],
        text_preview: None,
        citation_note: Some("Fixture citation".to_owned()),
        terms_note: Some("Fixture terms".to_owned()),
    }
}

fn worker_text_body() -> &'static str {
    "worker page one text\n\x0Cworker page two text\n"
}

struct SingleResponseFixtureServer {
    asset_url: String,
    _listener_thread: thread::JoinHandle<()>,
}

impl SingleResponseFixtureServer {
    fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let addr = listener.local_addr().expect("fixture server address");
        let listener_thread = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                read_http_request(&mut stream);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        Self {
            asset_url: format!("http://{addr}/fixture.txt"),
            _listener_thread: listener_thread,
        }
    }
}

fn read_http_request(stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut buffer = [0_u8; 512];
    while request.windows(4).all(|window| window != b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                request.extend_from_slice(&buffer[..read]);
                if request.len() > 16 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
