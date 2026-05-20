use crate::ingest::{
    ChunkOptions, ExtractedText, PageText, QueuedIngestionExecutor, TextExtraction, TextExtractor,
};
use crate::sources::{
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
    SourceFuture, SourceMetadata, SourceRecord, SourceStatus,
};
use crate::store::{ContentAddressedStore, NewIngestionJob, SqliteStore};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

const NARA_SOURCE_WARNING: &str =
    "NARA Catalog metadata requires record-level verification for OCR/transcripts and linked objects.";
const NARA_CITATION_NOTE: &str =
    "National Archives Catalog metadata. Verify digitized object links, OCR, and transcripts at source.";
const NARA_TERMS_NOTE: &str =
    "NARA Catalog API use requires an API key and has documented query limits. Persistent API response caching is disabled by default.";

#[derive(Clone)]
struct FakeNaraAdapter {
    record: SourceRecord,
}

impl SourceAdapter for FakeNaraAdapter {
    fn name(&self) -> &'static str {
        "nara"
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
                source: "nara_catalog",
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
        CachePolicy::DoNotPersist
    }
}

struct FixturePdfExtractor;

impl TextExtractor for FixturePdfExtractor {
    fn extract_pages(&self, _path: &std::path::Path) -> Result<ExtractedText, TextExtraction> {
        Ok(ExtractedText {
            pages: vec![
                PageText {
                    page_number: 1,
                    text: "nara page one fixture".to_owned(),
                },
                PageText {
                    page_number: 2,
                    text: "nara page two fixture".to_owned(),
                },
            ],
            warnings: vec!["nara fixture extractor warning".to_owned()],
        })
    }
}

fn nara_record(loopback_pdf_url: String) -> SourceRecord {
    let mut metadata = SourceMetadata::new();
    metadata.insert("naid".to_owned(), "595353".to_owned());
    metadata.insert("record_group".to_owned(), "rg-330".to_owned());
    metadata.insert(
        "cache_policy_note".to_owned(),
        "NARA adapter marks API fetches do-not-persist by default.".to_owned(),
    );
    metadata.insert("source_warning".to_owned(), NARA_SOURCE_WARNING.to_owned());

    SourceRecord {
        id: "nara:595353".to_owned(),
        document_key: "nara_595353".to_owned(),
        source: "nara",
        source_id: "595353".to_owned(),
        title: "Weather Modification Report".to_owned(),
        date: Some("1970-01-01".to_owned()),
        collection: Some("NARA Catalog".to_owned()),
        record_group: Some("RG-330".to_owned()),
        description: Some("NARA fixture metadata".to_owned()),
        origin_url: "https://catalog.archives.gov/id/595353".to_owned(),
        document_url: "https://catalog.archives.gov/id/595353".to_owned(),
        pdf_url: Some(loopback_pdf_url.clone()),
        metadata,
        attachments: vec![
            SourceAsset {
                asset_url: loopback_pdf_url,
                label: "Digital object PDF".to_owned(),
                mime_type: Some("application/pdf".to_owned()),
                role: SourceAssetRole::Pdf,
            },
            SourceAsset {
                asset_url: "https://catalog.archives.gov/files/595353/transcript.txt".to_owned(),
                label: "Transcript".to_owned(),
                mime_type: Some("text/plain".to_owned()),
                role: SourceAssetRole::Transcript,
            },
        ],
        text_preview: None,
        citation_note: Some(NARA_CITATION_NOTE.to_owned()),
        terms_note: Some(NARA_TERMS_NOTE.to_owned()),
    }
}

fn enqueue_nara_job(store: &mut SqliteStore) {
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: "ingest:nara:595353".to_owned(),
            operation: "ingest".to_owned(),
            source: "nara".to_owned(),
            source_id: Some("595353".to_owned()),
            target_url: None,
            next_action: "queued".to_owned(),
        })
        .expect("create NARA ingestion job");
}

#[tokio::test]
async fn nara_executor_loopback_keeps_do_not_persist_and_metadata_contracts() {
    let (loopback_pdf_url, request) = fixture_http_url(b"%PDF nara fixture");
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    let files_dir = tempfile::tempdir().expect("create files tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue_nara_job(&mut store);

    let executor = QueuedIngestionExecutor::new(
        "nara-worker",
        vec![Arc::new(FakeNaraAdapter {
            record: nara_record(loopback_pdf_url.clone()),
        })],
    )
    .expect("build NARA executor")
    .with_chunk_options(ChunkOptions { target_tokens: 4 });

    let (returned_store, run_result) = executor.run_next(store, &files, &FixturePdfExtractor).await;
    store = returned_store;
    let outcome = run_result
        .expect("NARA job execution should succeed")
        .expect("NARA job should be claimed");

    assert_eq!(outcome.document_key, "nara_595353");
    assert_eq!(outcome.page_count, 2);
    assert_eq!(outcome.chunk_count, 2);
    assert_eq!(
        outcome.warnings,
        vec!["nara fixture extractor warning".to_owned()]
    );

    let job = store
        .get_ingestion_job_record("ingest:nara:595353")
        .expect("load persisted job state");
    assert_eq!(job.status, "succeeded");
    assert_eq!(job.stage, "succeeded");
    assert_eq!(job.progress, 1.0);
    assert_eq!(
        job.warnings,
        vec!["nara fixture extractor warning".to_owned()]
    );

    let document = store
        .get_document_metadata("nara:595353")
        .expect("load persisted NARA document metadata");
    assert_eq!(document.source, "nara");
    assert_eq!(document.source_id, "595353");
    assert_eq!(document.page_count, 2);
    assert_eq!(
        document.origin_url.as_deref(),
        Some("https://catalog.archives.gov/id/595353")
    );
    assert_eq!(
        document.document_url.as_deref(),
        Some("https://catalog.archives.gov/id/595353")
    );
    assert_eq!(document.pdf_url.as_deref(), Some(loopback_pdf_url.as_str()));
    assert_eq!(document.citation_note.as_deref(), Some(NARA_CITATION_NOTE));
    assert_eq!(document.terms_note.as_deref(), Some(NARA_TERMS_NOTE));

    let metadata: Value =
        serde_json::from_str(&document.metadata_json).expect("document metadata_json is valid");
    assert_eq!(
        metadata["source_metadata"]["source_warning"],
        NARA_SOURCE_WARNING
    );
    assert_eq!(metadata["source_metadata"]["naid"], "595353");
    assert_eq!(metadata["ingest_plan"]["source"], "nara");
    assert_eq!(metadata["ingest_plan"]["source_id"], "595353");
    assert_eq!(metadata["ingest_plan"]["cache_policy"], "do_not_persist");
    assert_eq!(metadata["ingest_plan"]["selected_asset"]["role"], "pdf");
    assert_eq!(
        metadata["ingest_plan"]["selected_asset"]["text_source"],
        "embedded_pdf_text"
    );

    let pages = store
        .get_page_text("nara:595353", 1, 2)
        .expect("load persisted page text");
    assert_eq!(pages[0].text_source, "embedded_pdf_text");
    assert_eq!(pages[1].text_source, "embedded_pdf_text");

    let (asset_url, role, cache_policy): (String, String, String) = store
        .connection()
        .query_row(
            "
            SELECT a.asset_url, a.role, a.cache_policy
            FROM assets a
            JOIN documents d ON d.id = a.document_id
            WHERE d.public_id = ?1
            ",
            ["nara:595353"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load persisted asset row");
    assert_eq!(asset_url, loopback_pdf_url);
    assert_eq!(role, "pdf");
    assert_eq!(cache_policy, "do_not_persist");

    let cache_rows: i64 = store
        .connection()
        .query_row("SELECT count(*) FROM cache_entries", [], |row| row.get(0))
        .expect("count cache rows");
    assert_eq!(cache_rows, 0);

    let request = request.join().expect("fixture request capture");
    assert!(request.starts_with("GET /nara-fixture.pdf "));
}

fn fixture_http_url(body: &'static [u8]) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let address = listener.local_addr().expect("fixture server address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept fixture request");
        let request = read_http_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .expect("write response headers");
        stream.write_all(body).expect("write response body");
        request
    });
    (format!("http://{address}/nara-fixture.pdf"), handle)
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut buffer = [0_u8; 1024];
    let read = stream.read(&mut buffer).expect("read fixture request");
    String::from_utf8_lossy(&buffer[..read]).to_string()
}
