use crate::ingest::{
    ChunkOptions, ExtractedText, PageText, QueuedIngestionExecutor, TextExtraction, TextExtractor,
};
use crate::sources::{
    frus::{frus_citation_note, frus_terms_note, FrusAdapter},
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
    SourceFuture, SourceRecord, SourceStatus,
};
use crate::store::{ContentAddressedStore, NewIngestionJob, SqliteStore};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

const FRUS_SOURCE_WARNING: &str =
    "FRUS records should be cited from official history.state.gov document URLs with document-number context when available.";

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[derive(Clone)]
struct FakeFrusAdapter {
    record: SourceRecord,
}

impl SourceAdapter for FakeFrusAdapter {
    fn name(&self) -> &'static str {
        "frus"
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
                source: "history_state_gov_frus",
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

struct FixturePdfExtractor;

impl TextExtractor for FixturePdfExtractor {
    fn extract_pages(&self, _path: &std::path::Path) -> Result<ExtractedText, TextExtraction> {
        Ok(ExtractedText {
            pages: vec![
                PageText {
                    page_number: 1,
                    text: "frus page one fixture".to_owned(),
                },
                PageText {
                    page_number: 2,
                    text: "frus page two fixture".to_owned(),
                },
            ],
            warnings: vec!["frus fixture extractor warning".to_owned()],
        })
    }
}

async fn frus_record_from_fixture(loopback_pdf_url: &str) -> SourceRecord {
    let detail = include_str!("../../tests/fixtures/frus/detail_record.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        detail,
    )]);
    let adapter = FrusAdapter::new(base_url);
    let mut record = adapter
        .get_record("frus:frus1969-76v12/d34")
        .await
        .expect("parse FRUS fixture record");
    let captured = requests.join().expect("capture fixture request");
    assert_eq!(captured.len(), 1);

    for asset in &mut record.attachments {
        if asset.role == SourceAssetRole::Pdf {
            asset.asset_url = loopback_pdf_url.to_owned();
        }
    }
    record.pdf_url = Some(loopback_pdf_url.to_owned());
    record
}

fn enqueue_frus_job(store: &mut SqliteStore) {
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: "ingest:frus:frus1969-76v12/d34".to_owned(),
            operation: "ingest".to_owned(),
            source: "frus".to_owned(),
            source_id: Some("frus1969-76v12/d34".to_owned()),
            target_url: None,
            next_action: "queued".to_owned(),
        })
        .expect("create FRUS ingestion job");
}

#[tokio::test]
async fn frus_executor_loopback_preserves_official_metadata_and_notes() {
    let (loopback_pdf_url, request) = fixture_http_url(b"%PDF frus fixture");
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    let files_dir = tempfile::tempdir().expect("create files tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue_frus_job(&mut store);

    let record = frus_record_from_fixture(&loopback_pdf_url).await;
    assert_eq!(record.id, "frus:frus1969-76v12/d34");

    let executor =
        QueuedIngestionExecutor::new("frus-worker", vec![Arc::new(FakeFrusAdapter { record })])
            .expect("build FRUS executor")
            .with_chunk_options(ChunkOptions { target_tokens: 4 });

    let (returned_store, run_result) = executor.run_next(store, &files, &FixturePdfExtractor).await;
    store = returned_store;
    let outcome = run_result
        .expect("FRUS job execution should succeed")
        .expect("FRUS job should be claimed");

    assert_eq!(outcome.document_key, "frus-frus1969-76v12-d34");
    assert_eq!(outcome.page_count, 2);
    assert_eq!(outcome.chunk_count, 2);
    assert_eq!(
        outcome.warnings,
        vec!["frus fixture extractor warning".to_owned()]
    );

    let job = store
        .get_ingestion_job_record("ingest:frus:frus1969-76v12/d34")
        .expect("load persisted job state");
    assert_eq!(job.status, "succeeded");
    assert_eq!(job.stage, "succeeded");
    assert_eq!(job.progress, 1.0);

    let document = store
        .get_document_metadata("frus:frus1969-76v12/d34")
        .expect("load persisted FRUS document metadata");
    assert_eq!(document.source, "frus");
    assert_eq!(document.source_id, "frus1969-76v12/d34");
    assert_eq!(document.page_count, 2);
    assert!(document
        .origin_url
        .as_deref()
        .is_some_and(|url| url.starts_with("http://127.0.0.1:")));
    assert!(document
        .origin_url
        .as_deref()
        .is_some_and(|url| url.ends_with("/historicaldocuments/frus1969-76v12/d34")));
    assert_eq!(
        document.document_url.as_deref(),
        Some("https://history.state.gov/historicaldocuments/frus1969-76v12/d34")
    );
    assert_eq!(document.pdf_url.as_deref(), Some(loopback_pdf_url.as_str()));
    assert_eq!(
        document.citation_note.as_deref(),
        Some(frus_citation_note())
    );
    assert_eq!(document.terms_note.as_deref(), Some(frus_terms_note()));

    let metadata: Value =
        serde_json::from_str(&document.metadata_json).expect("document metadata_json is valid");
    assert_eq!(
        metadata["source_metadata"]["source_warning"],
        FRUS_SOURCE_WARNING
    );
    assert_eq!(
        metadata["source_metadata"]["official_document_url"],
        "https://history.state.gov/historicaldocuments/frus1969-76v12/d34"
    );
    assert_eq!(
        metadata["source_metadata"]["tei_xml_url"],
        "https://raw.githubusercontent.com/HistoryAtState/frus/master/volumes/frus1969-76v12.xml"
    );
    assert_eq!(
        metadata["source_metadata"]["pdf_url"],
        "https://static.history.state.gov/frus/frus1969-76v12/pdf/frus1969-76v12.pdf"
    );
    assert_eq!(
        metadata["ingest_plan"]["cache_policy"],
        "respect_source_headers"
    );
    assert_eq!(metadata["ingest_plan"]["selected_asset"]["role"], "pdf");
    assert_eq!(
        metadata["ingest_plan"]["selected_asset"]["text_source"],
        "embedded_pdf_text"
    );

    let pages = store
        .get_page_text("frus:frus1969-76v12/d34", 1, 2)
        .expect("load persisted page text");
    assert_eq!(pages[0].text_source, "embedded_pdf_text");
    assert_eq!(pages[1].text_source, "embedded_pdf_text");

    let request = request.join().expect("fixture request capture");
    assert!(request.starts_with("GET /frus-fixture.pdf "));
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
    (format!("http://{address}/frus-fixture.pdf"), handle)
}

fn serve_sequence(responses: Vec<ResponseSpec>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture server should bind");
    let addr = listener.local_addr().expect("fixture server address");

    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status_line, headers, body) in responses {
            let (mut stream, _) = listener.accept().expect("fixture server should accept");
            let mut buffer = [0_u8; 8192];
            let read = stream
                .read(&mut buffer)
                .expect("fixture server should read");
            requests.push(String::from_utf8_lossy(&buffer[..read]).to_string());

            let mut header_block = String::new();
            for (name, value) in headers {
                header_block.push_str(name);
                header_block.push_str(": ");
                header_block.push_str(value);
                header_block.push_str("\r\n");
            }
            let response = format!(
                "{status_line}\r\nContent-Length: {}\r\n{header_block}Connection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("fixture server should write");
        }
        requests
    });

    (format!("http://{addr}"), handle)
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut buffer = [0_u8; 1024];
    let read = stream.read(&mut buffer).expect("read fixture request");
    String::from_utf8_lossy(&buffer[..read]).to_string()
}
