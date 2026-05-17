use crate::ingest::{
    ExecutorError, ExecutorJobOutcome, NoopOcrExtractor, OcrBackend, OcrBackendConfig,
    OcrFallbackPolicy, OcrmypdfConfig, OcrmypdfExtractor, PdftotextExtractor,
    QueuedIngestionExecutor, TextExtraction, TextExtractor,
};
use crate::sources::SourceAdapter;
use crate::store::{ContentAddressedStore, SqliteStore, StoreError};
use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct QueuedIngestionWorker {
    data_dir: PathBuf,
    sources: Vec<Arc<dyn SourceAdapter>>,
    poll_interval: Duration,
    ocr_policy: OcrFallbackPolicy,
    ocr_backend: OcrBackendConfig,
}

#[derive(Debug)]
pub enum WorkerError {
    Io(std::io::Error),
    Store(StoreError),
    Executor(ExecutorError),
}

pub struct IngestionWorkerHandle {
    control: Option<mpsc::Sender<WorkerCommand>>,
    join: Option<thread::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct IngestionWorkerKick {
    control: mpsc::Sender<WorkerCommand>,
}

#[derive(Debug)]
pub enum WorkerKickError {
    Stopped,
}

#[derive(Clone, Copy, Debug)]
enum WorkerCommand {
    Kick,
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerWait {
    Kick,
    Shutdown,
    Timeout,
}

impl QueuedIngestionWorker {
    pub fn new(data_dir: impl Into<PathBuf>, sources: Vec<Arc<dyn SourceAdapter>>) -> Self {
        Self {
            data_dir: data_dir.into(),
            sources,
            poll_interval: DEFAULT_POLL_INTERVAL,
            ocr_policy: OcrFallbackPolicy::off(),
            ocr_backend: OcrBackendConfig::default(),
        }
    }

    pub fn with_ocr_policy(mut self, ocr_policy: OcrFallbackPolicy) -> Self {
        self.ocr_policy = ocr_policy;
        self
    }

    pub fn with_ocr_backend(mut self, ocr_backend: OcrBackendConfig) -> Self {
        self.ocr_backend = ocr_backend;
        self
    }

    pub fn spawn(self) -> IngestionWorkerHandle {
        let (control, control_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            self.run_until_shutdown(control_rx);
        });
        IngestionWorkerHandle {
            control: Some(control),
            join: Some(join),
        }
    }

    pub async fn run_once(&self) -> Result<Option<ExecutorJobOutcome>, WorkerError> {
        let pdf_extractor = PdftotextExtractor::default();
        let ocr_extractor = worker_ocr_extractor(self.ocr_policy, &self.ocr_backend);
        self.run_once_with_extractors(&pdf_extractor, &ocr_extractor)
            .await
    }

    #[cfg(test)]
    async fn run_once_with_extractor(
        &self,
        pdf_extractor: &dyn TextExtractor,
    ) -> Result<Option<ExecutorJobOutcome>, WorkerError> {
        self.run_once_with_extractors(pdf_extractor, &NoopOcrExtractor)
            .await
    }

    async fn run_once_with_extractors(
        &self,
        pdf_extractor: &dyn TextExtractor,
        ocr_extractor: &dyn TextExtractor,
    ) -> Result<Option<ExecutorJobOutcome>, WorkerError> {
        let mut store = self.open_store()?;
        let files = ContentAddressedStore::new(&self.data_dir);
        let executor = QueuedIngestionExecutor::new("foia-ingest-worker", self.sources.clone())?
            .with_ocr_policy(self.ocr_policy);
        executor
            .run_next_with_ocr(&mut store, &files, pdf_extractor, ocr_extractor)
            .await
            .map_err(WorkerError::from)
    }

    fn run_until_shutdown(self, control: mpsc::Receiver<WorkerCommand>) {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::warn!(error = %error, "queued ingestion worker runtime failed to start");
                return;
            }
        };

        loop {
            if shutdown_requested(&control) {
                break;
            }
            match runtime.block_on(self.run_once()) {
                Ok(Some(outcome)) => {
                    tracing::info!(
                        job_key = %outcome.job_key,
                        document_key = %outcome.document_key,
                        page_count = outcome.page_count,
                        chunk_count = outcome.chunk_count,
                        "queued ingestion job completed"
                    );
                }
                Ok(None) => {
                    if wait_for_next_iteration(&control, self.poll_interval) == WorkerWait::Shutdown
                    {
                        break;
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "queued ingestion worker iteration failed");
                    if wait_for_next_iteration(&control, self.poll_interval) == WorkerWait::Shutdown
                    {
                        break;
                    }
                }
            }
        }
    }

    fn open_store(&self) -> Result<SqliteStore, WorkerError> {
        let db_dir = self.data_dir.join("db");
        std::fs::create_dir_all(&db_dir)?;
        Ok(SqliteStore::open(db_dir.join("foia.sqlite"))?)
    }
}

enum WorkerOcrExtractor {
    Noop(NoopOcrExtractor),
    Ocrmypdf(OcrmypdfExtractor),
}

impl TextExtractor for WorkerOcrExtractor {
    fn extract_pages(
        &self,
        path: &std::path::Path,
    ) -> Result<crate::ingest::ExtractedText, TextExtraction> {
        match self {
            Self::Noop(extractor) => extractor.extract_pages(path),
            Self::Ocrmypdf(extractor) => extractor.extract_pages(path),
        }
    }
}

fn worker_ocr_extractor(
    policy: OcrFallbackPolicy,
    backend_config: &OcrBackendConfig,
) -> WorkerOcrExtractor {
    match effective_ocr_backend(policy, backend_config) {
        OcrBackend::Ocrmypdf => {
            WorkerOcrExtractor::Ocrmypdf(OcrmypdfExtractor::new(OcrmypdfConfig::new(
                backend_config.ocrmypdf_binary.clone(),
                backend_config.timeout,
                backend_config.max_stderr_bytes,
            )))
        }
        OcrBackend::None => WorkerOcrExtractor::Noop(NoopOcrExtractor),
    }
}

fn effective_ocr_backend(
    policy: OcrFallbackPolicy,
    backend_config: &OcrBackendConfig,
) -> OcrBackend {
    if policy.is_enabled() && backend_config.backend.is_enabled() {
        backend_config.backend
    } else {
        OcrBackend::None
    }
}

impl IngestionWorkerHandle {
    pub fn kick_handle(&self) -> Option<IngestionWorkerKick> {
        self.control
            .as_ref()
            .cloned()
            .map(|control| IngestionWorkerKick { control })
    }

    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self
            .control
            .take()
            .map(|control| control.send(WorkerCommand::Shutdown));
        let _ = self.join.take().map(|join| join.join());
    }
}

impl IngestionWorkerKick {
    pub fn kick(&self) -> Result<(), WorkerKickError> {
        self.control
            .send(WorkerCommand::Kick)
            .map_err(|_| WorkerKickError::Stopped)
    }
}

impl Drop for IngestionWorkerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn shutdown_requested(control: &mpsc::Receiver<WorkerCommand>) -> bool {
    loop {
        match control.try_recv() {
            Ok(WorkerCommand::Kick) => continue,
            Ok(WorkerCommand::Shutdown) | Err(TryRecvError::Disconnected) => return true,
            Err(TryRecvError::Empty) => return false,
        }
    }
}

fn wait_for_next_iteration(
    control: &mpsc::Receiver<WorkerCommand>,
    timeout: Duration,
) -> WorkerWait {
    match control.recv_timeout(timeout) {
        Ok(WorkerCommand::Kick) => WorkerWait::Kick,
        Ok(WorkerCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => WorkerWait::Shutdown,
        Err(RecvTimeoutError::Timeout) => WorkerWait::Timeout,
    }
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "queued ingestion worker I/O error: {err}"),
            Self::Store(err) => write!(f, "{err}"),
            Self::Executor(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for WorkerError {}

impl From<std::io::Error> for WorkerError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<StoreError> for WorkerError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

impl From<ExecutorError> for WorkerError {
    fn from(err: ExecutorError) -> Self {
        Self::Executor(err)
    }
}

impl fmt::Display for WorkerKickError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stopped => write!(f, "queued ingestion worker is stopped"),
        }
    }
}

impl std::error::Error for WorkerKickError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{ExtractedText, PageText, TextExtraction};
    use crate::sources::{
        CachePolicy, SearchOptions, SearchPage, SourceAsset, SourceAssetRole, SourceFuture,
        SourceMetadata, SourceRecord, SourceStatus,
    };
    use crate::store::NewIngestionJob;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
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
            Box::pin(async move { unreachable!("worker tests do not search") })
        }

        fn get_record<'a>(&'a self, _id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
            Box::pin(async move { Ok(self.record.clone()) })
        }

        fn list_assets<'a>(
            &'a self,
            record: &'a SourceRecord,
        ) -> SourceFuture<'a, Vec<SourceAsset>> {
            Box::pin(async move { Ok(record.attachments.clone()) })
        }

        fn cache_policy(&self) -> CachePolicy {
            CachePolicy::RespectSourceHeaders
        }
    }

    struct FakePdfExtractor;

    impl TextExtractor for FakePdfExtractor {
        fn extract_pages(&self, _path: &std::path::Path) -> Result<ExtractedText, TextExtraction> {
            Ok(ExtractedText {
                pages: vec![PageText {
                    page_number: 1,
                    text: "queued worker fixture text".to_owned(),
                }],
                warnings: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn run_once_picks_up_queued_job_after_enqueue() {
        let temp = tempfile::tempdir().expect("tempdir");
        enqueue(temp.path());
        let worker = QueuedIngestionWorker::new(
            temp.path(),
            vec![Arc::new(FakeAdapter {
                record: source_record(fixture_http_url(b"%PDF worker body")),
            })],
        );

        let outcome = worker
            .run_once_with_extractor(&FakePdfExtractor)
            .await
            .expect("worker run")
            .expect("queued job");

        assert_eq!(outcome.job_key, "ingest:cia:CREST-worker");
        assert_eq!(outcome.page_count, 1);
        assert_eq!(job_status(temp.path()), ("succeeded".to_owned(), 1));
    }

    #[tokio::test]
    async fn run_once_returns_none_when_no_job_is_available() {
        let temp = tempfile::tempdir().expect("tempdir");
        let worker = QueuedIngestionWorker::new(temp.path(), Vec::new());

        let outcome = worker
            .run_once_with_extractor(&FakePdfExtractor)
            .await
            .expect("idle run");

        assert!(outcome.is_none());
    }

    #[tokio::test]
    async fn run_once_marks_claimed_job_failed_when_execution_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        enqueue(temp.path());
        let worker = QueuedIngestionWorker::new(temp.path(), Vec::new());

        let error = worker
            .run_once_with_extractor(&FakePdfExtractor)
            .await
            .expect_err("missing source should fail");

        assert!(error.to_string().contains("no source adapter registered"));
        let (status, attempts) = job_status(temp.path());
        assert_eq!(status, "failed");
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn completed_job_is_not_executed_again() {
        let temp = tempfile::tempdir().expect("tempdir");
        enqueue(temp.path());
        let worker = QueuedIngestionWorker::new(
            temp.path(),
            vec![Arc::new(FakeAdapter {
                record: source_record(fixture_http_url(b"%PDF worker body")),
            })],
        );

        let first = worker
            .run_once_with_extractor(&FakePdfExtractor)
            .await
            .expect("first run");
        let second = worker
            .run_once_with_extractor(&FakePdfExtractor)
            .await
            .expect("second run");

        assert!(first.is_some());
        assert!(second.is_none());
        assert_eq!(job_status(temp.path()), ("succeeded".to_owned(), 1));
    }

    #[test]
    fn kick_wakes_idle_worker_wait_without_poll_timeout() {
        let (kick, control) = mpsc::channel();
        kick.send(WorkerCommand::Kick).expect("send kick");

        let wait = wait_for_next_iteration(&control, Duration::from_secs(60));

        assert_eq!(wait, WorkerWait::Kick);
    }

    #[test]
    fn idle_worker_wait_times_out_without_kick() {
        let (_kick, control) = mpsc::channel();

        let wait = wait_for_next_iteration(&control, Duration::from_millis(1));

        assert_eq!(wait, WorkerWait::Timeout);
    }

    #[test]
    fn shutdown_takes_precedence_over_drained_kicks() {
        let (kick, control) = mpsc::channel();
        kick.send(WorkerCommand::Kick).expect("send kick");
        kick.send(WorkerCommand::Shutdown).expect("send shutdown");

        assert!(shutdown_requested(&control));
    }

    #[test]
    fn effective_ocr_backend_requires_policy_and_backend_opt_in() {
        let backend = OcrBackendConfig {
            backend: OcrBackend::Ocrmypdf,
            ..OcrBackendConfig::default()
        };

        assert_eq!(
            effective_ocr_backend(OcrFallbackPolicy::off(), &backend),
            OcrBackend::None
        );
        assert_eq!(
            effective_ocr_backend(
                OcrFallbackPolicy::on_quality_warning(),
                &OcrBackendConfig::default()
            ),
            OcrBackend::None
        );
        assert_eq!(
            effective_ocr_backend(OcrFallbackPolicy::on_quality_warning(), &backend),
            OcrBackend::Ocrmypdf
        );
    }

    fn enqueue(data_dir: &std::path::Path) {
        let mut store = open_store(data_dir);
        store
            .create_ingestion_job(&NewIngestionJob {
                job_key: "ingest:cia:CREST-worker".to_owned(),
                operation: "ingest".to_owned(),
                source: "cia".to_owned(),
                source_id: Some("CREST-worker".to_owned()),
                target_url: None,
                next_action: "queued".to_owned(),
            })
            .expect("create job");
    }

    fn job_status(data_dir: &std::path::Path) -> (String, u32) {
        let store = open_store(data_dir);
        let job = store
            .get_ingestion_job_record("ingest:cia:CREST-worker")
            .expect("job status");
        (job.status, job.attempts)
    }

    fn open_store(data_dir: &std::path::Path) -> SqliteStore {
        let db_dir = data_dir.join("db");
        std::fs::create_dir_all(&db_dir).expect("db dir");
        SqliteStore::open(db_dir.join("foia.sqlite")).expect("store")
    }

    fn source_record(asset_url: String) -> SourceRecord {
        SourceRecord {
            id: "cia:CREST-worker".to_owned(),
            document_key: "cia_CREST-worker".to_owned(),
            source: "cia",
            source_id: "CREST-worker".to_owned(),
            title: "Worker Fixture".to_owned(),
            date: None,
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("worker test".to_owned()),
            origin_url: "https://www.cia.gov/readingroom/document/CREST-worker".to_owned(),
            document_url: "https://www.cia.gov/readingroom/document/CREST-worker".to_owned(),
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
}
