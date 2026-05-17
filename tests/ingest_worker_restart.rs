mod ingest_worker_restart_support;

use foia_search::{
    ingest::QueuedIngestionWorker,
    sources::{
        CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
        SourceFuture, SourceMetadata, SourceRecord, SourceStatus,
    },
    store::{NewIngestionJob, SqliteStore},
};
use ingest_worker_restart_support::{
    chunk_fts_match_count, count_text_rows_with_substring, seed_partial_local_state,
    SingleResponseFixtureServer,
};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const JOB_KEY: &str = "ingest:cia:CREST-restart";
const CHILD_MODE_ENV: &str = "FOIA_RESTART_CHILD_MODE";
const CHILD_DATA_DIR_ENV: &str = "FOIA_RESTART_DATA_DIR";
const CHILD_ASSET_URL_ENV: &str = "FOIA_RESTART_ASSET_URL";
const CHILD_MODE_FIRST: &str = "first";
const CHILD_MODE_RESUME: &str = "resume";
const CHILD_MODE_SEED_PARTIAL: &str = "seed-partial";
const CHILD_MODE_RESUME_PARTIAL: &str = "resume-partial";
const TEST_NAME: &str =
    "queued_worker_process_restart_resumes_expired_running_job_without_duplicates";
const TEST_NAME_PARTIAL: &str =
    "queued_worker_process_restart_replaces_seeded_partial_local_state_without_duplicates";
const FIRST_REQUEST_SEEN_FILE: &str = "first-request-seen";
const RELEASE_FIRST_RESPONSE_FILE: &str = "release-first-response";
const LEASE_EXPIRY: &str = "1970-01-01T00:00:00.000Z";

#[test]
fn queued_worker_process_restart_resumes_expired_running_job_without_duplicates() {
    if let Some(mode) = ChildMode::from_env() {
        run_child(mode).expect("child run should execute deterministically");
        return;
    }

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let data_dir = tempdir.path().join("data");
    let control_dir = tempdir.path().join("control");
    fs::create_dir_all(&data_dir).expect("create data dir");
    fs::create_dir_all(&control_dir).expect("create control dir");

    let first_request_seen = control_dir.join(FIRST_REQUEST_SEEN_FILE);
    let release_first_response = control_dir.join(RELEASE_FIRST_RESPONSE_FILE);
    let server = ControlledFixtureServer::start(
        first_request_seen.clone(),
        release_first_response.clone(),
        fixture_text_body(),
    );

    enqueue_job(&data_dir);

    let mut first_child =
        spawn_child_process(TEST_NAME, CHILD_MODE_FIRST, &data_dir, &server.asset_url);
    assert!(
        wait_for_path(&first_request_seen, Duration::from_secs(15)),
        "first child should reach blocked download request"
    );
    wait_for_condition(
        Duration::from_secs(5),
        "job should be running in downloading_asset stage",
        || {
            let store = open_store(&data_dir);
            let job = store
                .get_ingestion_job_record(JOB_KEY)
                .expect("load running job");
            job.status == "running" && job.stage == "downloading_asset"
        },
    );

    first_child.kill().expect("kill first child process");
    let first_status = first_child.wait().expect("wait first child");
    assert!(
        !first_status.success(),
        "killed first child should not exit successfully"
    );

    {
        let store = open_store(&data_dir);
        let running = store
            .get_ingestion_job_record(JOB_KEY)
            .expect("load running record after crash");
        assert_eq!(running.status, "running");
        assert_eq!(running.stage, "downloading_asset");
        assert_eq!(running.attempts, 1);
        assert!(
            running.progress >= 0.35,
            "progress should remain at or past downloading stage"
        );
        store
            .connection()
            .execute(
                "
                UPDATE ingestion_jobs
                SET lease_expires_at = ?2
                WHERE job_key = ?1
                ",
                (JOB_KEY, LEASE_EXPIRY),
            )
            .expect("expire running lease");
    }

    let mut resume_child =
        spawn_child_process(TEST_NAME, CHILD_MODE_RESUME, &data_dir, &server.asset_url);
    let resume_status = resume_child.wait().expect("wait resume child");
    assert!(
        resume_status.success(),
        "resume child should complete ingestion after reclaim"
    );

    fs::write(&release_first_response, "release").expect("release blocked request handler");

    let store = open_store(&data_dir);
    let finished = store
        .get_ingestion_job_record(JOB_KEY)
        .expect("load completed job");
    assert_eq!(finished.status, "succeeded");
    assert_eq!(finished.stage, "succeeded");
    assert_eq!(finished.progress, 1.0);
    assert_eq!(finished.attempts, 2);
    assert!(finished.error.is_none());

    assert_eq!(row_count(&store, "documents"), 1);
    assert_eq!(row_count(&store, "assets"), 1);
    assert_eq!(row_count(&store, "pages"), 3);
    assert_eq!(row_count(&store, "chunks"), 1);
    assert_eq!(row_count(&store, "chunk_fts"), 1);

    let pages = store
        .get_page_text("cia:CREST-restart", 1, 3)
        .expect("page text after resume");
    assert_eq!(pages.len(), 3);
    assert!(pages[0].text.contains("alpha page one"));
    assert!(pages[1].text.contains("bravo page two"));
    assert!(pages[2].text.contains("charlie page three"));
}

#[test]
fn queued_worker_process_restart_replaces_seeded_partial_local_state_without_duplicates() {
    if let Some(mode) = ChildMode::from_env() {
        run_child(mode).expect("child run should execute deterministically");
        return;
    }

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let data_dir = tempdir.path().join("data");
    fs::create_dir_all(&data_dir).expect("create data dir");

    enqueue_job(&data_dir);
    let server = SingleResponseFixtureServer::start(fixture_text_body());

    let seed_child = spawn_child_process(
        TEST_NAME_PARTIAL,
        CHILD_MODE_SEED_PARTIAL,
        &data_dir,
        &server.asset_url,
    );
    let seed_status = seed_child.wait_with_output().expect("wait seeded child");
    assert!(
        seed_status.status.success(),
        "seed child should persist stale local rows and stage/progress state"
    );

    {
        let store = open_store(&data_dir);
        let running = store
            .get_ingestion_job_record(JOB_KEY)
            .expect("load running record after seed child");
        assert_eq!(running.status, "running");
        assert_eq!(running.stage, "extracting_text");
        assert_eq!(running.attempts, 1);
        assert_eq!(running.progress, 0.60);
        assert_eq!(running.lease_owner.as_deref(), Some("seed-worker"));
        assert_eq!(row_count(&store, "documents"), 1);
        assert_eq!(row_count(&store, "assets"), 1);
        assert_eq!(row_count(&store, "pages"), 1);
        assert_eq!(row_count(&store, "chunks"), 1);
        assert_eq!(row_count(&store, "chunk_fts"), 1);
        assert_eq!(count_text_rows_with_substring(&store, "pages", "stale"), 1);
        assert_eq!(count_text_rows_with_substring(&store, "chunks", "stale"), 1);
    }

    let mut resume_child = spawn_child_process(
        TEST_NAME_PARTIAL,
        CHILD_MODE_RESUME_PARTIAL,
        &data_dir,
        &server.asset_url,
    );
    let resume_status = resume_child.wait().expect("wait resume child");
    assert!(
        resume_status.success(),
        "resume child should replace stale partial local persistence"
    );

    let store = open_store(&data_dir);
    let finished = store
        .get_ingestion_job_record(JOB_KEY)
        .expect("load completed job");
    assert_eq!(finished.status, "succeeded");
    assert_eq!(finished.stage, "succeeded");
    assert_eq!(finished.progress, 1.0);
    assert_eq!(finished.attempts, 2);
    assert!(finished.error.is_none());

    assert_eq!(row_count(&store, "documents"), 1);
    assert_eq!(row_count(&store, "assets"), 1);
    assert_eq!(row_count(&store, "pages"), 3);
    assert_eq!(row_count(&store, "chunks"), 1);
    assert_eq!(row_count(&store, "chunk_fts"), 1);
    assert_eq!(count_text_rows_with_substring(&store, "pages", "stale"), 0);
    assert_eq!(count_text_rows_with_substring(&store, "chunks", "stale"), 0);
    assert_eq!(chunk_fts_match_count(&store, "stale"), 0);
    assert_eq!(chunk_fts_match_count(&store, "charlie"), 1);

    let pages = store
        .get_page_text("cia:CREST-restart", 1, 3)
        .expect("page text after resume");
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0].text, "alpha page one text");
    assert_eq!(pages[1].text, "bravo page two text");
    assert_eq!(pages[2].text, "charlie page three text");

    let chunk_text: String = store
        .connection()
        .query_row("SELECT text FROM chunks LIMIT 1", [], |row| row.get(0))
        .expect("load chunk text");
    assert!(chunk_text.contains("alpha page one text"));
    assert!(!chunk_text.contains("stale"));

    let description: String = store
        .connection()
        .query_row(
            "SELECT description FROM documents WHERE public_id = ?1",
            ["cia:CREST-restart"],
            |row| row.get(0),
        )
        .expect("load document description");
    assert_eq!(description, "Process-boundary restart fixture");
}

#[derive(Clone, Copy)]
enum ChildMode {
    First,
    Resume,
    SeedPartial,
    ResumePartial,
}

impl ChildMode {
    fn from_env() -> Option<Self> {
        match std::env::var(CHILD_MODE_ENV).ok()?.as_str() {
            CHILD_MODE_FIRST => Some(Self::First),
            CHILD_MODE_RESUME => Some(Self::Resume),
            CHILD_MODE_SEED_PARTIAL => Some(Self::SeedPartial),
            CHILD_MODE_RESUME_PARTIAL => Some(Self::ResumePartial),
            _ => None,
        }
    }
}

fn run_child(mode: ChildMode) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var(CHILD_DATA_DIR_ENV)?;
    let asset_url = std::env::var(CHILD_ASSET_URL_ENV)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    match mode {
        ChildMode::First => {
            let worker = QueuedIngestionWorker::new(
                data_dir.clone(),
                vec![Arc::new(FakeAdapter {
                    record: source_record(asset_url.clone()),
                })],
            );
            let _ = runtime.block_on(worker.run_once())?;
            Err("first child should be terminated by parent during blocked download".into())
        }
        ChildMode::Resume => {
            let worker = QueuedIngestionWorker::new(
                data_dir.clone(),
                vec![Arc::new(FakeAdapter {
                    record: source_record(asset_url.clone()),
                })],
            );
            let outcome = runtime.block_on(worker.run_once())?;
            if outcome.is_some() {
                Ok(())
            } else {
                Err("resume child did not find a claimable ingestion job".into())
            }
        }
        ChildMode::SeedPartial => {
            seed_partial_local_state(Path::new(&data_dir), JOB_KEY, LEASE_EXPIRY, &asset_url)
                .expect("seed stale local state");
            Ok(())
        }
        ChildMode::ResumePartial => {
            let worker = QueuedIngestionWorker::new(
                data_dir,
                vec![Arc::new(FakeAdapter {
                    record: source_record(asset_url),
                })],
            );
            let outcome = runtime.block_on(worker.run_once())?;
            if outcome.is_some() {
                Ok(())
            } else {
                Err("resume partial child did not find a claimable ingestion job".into())
            }
        }
    }
}

fn enqueue_job(data_dir: &Path) {
    let mut store = open_store(data_dir);
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: JOB_KEY.to_owned(),
            operation: "ingest".to_owned(),
            source: "cia".to_owned(),
            source_id: Some("CREST-restart".to_owned()),
            target_url: None,
            next_action: "queued".to_owned(),
        })
        .expect("create ingestion job");
}

fn open_store(data_dir: &Path) -> SqliteStore {
    let db_dir = data_dir.join("db");
    fs::create_dir_all(&db_dir).expect("create db dir");
    SqliteStore::open(db_dir.join("foia.sqlite")).expect("open sqlite store")
}

fn spawn_child_process(test_name: &str, mode: &str, data_dir: &Path, asset_url: &str) -> Child {
    Command::new(std::env::current_exe().expect("resolve current test executable"))
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(CHILD_MODE_ENV, mode)
        .env(CHILD_DATA_DIR_ENV, data_dir)
        .env(CHILD_ASSET_URL_ENV, asset_url)
        .env("RUST_TEST_THREADS", "1")
        .spawn()
        .expect("spawn child test process")
}

fn wait_for_condition<F>(timeout: Duration, description: &str, mut predicate: F)
where
    F: FnMut() -> bool,
{
    let start = Instant::now();
    while start.elapsed() < timeout {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out: {description}");
}

fn row_count(store: &SqliteStore, table: &str) -> i64 {
    store
        .connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count table rows")
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
        Box::pin(async move { unreachable!("restart test does not call search") })
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

fn source_record(asset_url: String) -> SourceRecord {
    SourceRecord {
        id: "cia:CREST-restart".to_owned(),
        document_key: "cia_CREST-restart".to_owned(),
        source: "cia",
        source_id: "CREST-restart".to_owned(),
        title: "Restart Fixture".to_owned(),
        date: Some("1962-08-01".to_owned()),
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("Process-boundary restart fixture".to_owned()),
        origin_url: "https://www.cia.gov/readingroom/document/CREST-restart".to_owned(),
        document_url: "https://www.cia.gov/readingroom/document/CREST-restart".to_owned(),
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

fn fixture_text_body() -> &'static str {
    "alpha page one text\n\x0Cbravo page two text\n\x0Ccharlie page three text\n"
}

struct ControlledFixtureServer {
    asset_url: String,
    _listener_thread: thread::JoinHandle<()>,
}

impl ControlledFixtureServer {
    fn start(
        first_request_seen: PathBuf,
        release_first_response: PathBuf,
        body: &'static str,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let addr = listener.local_addr().expect("fixture server address");
        let listener_thread = thread::spawn(move || {
            let mut accepted = 0_usize;
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    break;
                };
                accepted += 1;
                let first_request_seen = first_request_seen.clone();
                let release_first_response = release_first_response.clone();
                let response_body = body;
                let request_number = accepted;
                thread::spawn(move || {
                    handle_connection(
                        stream,
                        request_number,
                        &first_request_seen,
                        &release_first_response,
                        response_body,
                    );
                });
                if accepted >= 2 {
                    break;
                }
            }
        });

        Self {
            asset_url: format!("http://{addr}/fixture.txt"),
            _listener_thread: listener_thread,
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    request_number: usize,
    first_request_seen: &Path,
    release_first_response: &Path,
    body: &str,
) {
    read_http_request(&mut stream);
    if request_number == 1 {
        let _ = fs::write(first_request_seen, "seen");
        let _ = wait_for_path(release_first_response, Duration::from_secs(30));
    }

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    false
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
