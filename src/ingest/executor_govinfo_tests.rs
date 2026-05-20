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

const GOVINFO_SOURCE_WARNING: &str = "GovInfo metadata can include package-level and granule-level links; verify publication context and cited pages against the official GovInfo details page and selected official asset.";
const GOVINFO_CACHE_POLICY_NOTE: &str =
    "GovInfo API responses and downloaded assets respect source-provided cache headers.";
const GOVINFO_REDIRECT_POLICY_NOTE: &str =
    "GovInfo source fetches deny redirects by default unless an adapter-specific policy is explicitly reviewed.";

#[derive(Clone)]
struct FakeGovInfoAdapter {
    record: SourceRecord,
}

impl SourceAdapter for FakeGovInfoAdapter {
    fn name(&self) -> &'static str {
        "govinfo"
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
                source: "govinfo_search_service",
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
                    text: "govinfo page one fixture".to_owned(),
                },
                PageText {
                    page_number: 2,
                    text: "govinfo page two fixture".to_owned(),
                },
            ],
            warnings: vec!["govinfo fixture extractor warning".to_owned()],
        })
    }
}

fn govinfo_record(loopback_pdf_url: String) -> SourceRecord {
    let mut metadata = SourceMetadata::new();
    metadata.insert("collectionCode".to_owned(), "USREPORTS".to_owned());
    metadata.insert(
        "source_warning".to_owned(),
        GOVINFO_SOURCE_WARNING.to_owned(),
    );
    metadata.insert(
        "cache_policy_note".to_owned(),
        GOVINFO_CACHE_POLICY_NOTE.to_owned(),
    );
    metadata.insert(
        "redirect_policy_note".to_owned(),
        GOVINFO_REDIRECT_POLICY_NOTE.to_owned(),
    );

    SourceRecord {
        id: "govinfo:USREPORTS-99".to_owned(),
        document_key: "govinfo_usreports_99".to_owned(),
        source: "govinfo",
        source_id: "USREPORTS-99".to_owned(),
        title: "United States Reports Volume 99".to_owned(),
        date: Some("1880-01-01".to_owned()),
        collection: Some("USREPORTS".to_owned()),
        record_group: None,
        description: Some("GovInfo package fixture".to_owned()),
        origin_url: "https://www.govinfo.gov/app/details/USREPORTS-99".to_owned(),
        document_url: "https://api.govinfo.gov/packages/USREPORTS-99/summary".to_owned(),
        pdf_url: Some(loopback_pdf_url.clone()),
        metadata,
        attachments: vec![
            SourceAsset {
                asset_url: loopback_pdf_url,
                label: "PDF".to_owned(),
                mime_type: Some("application/pdf".to_owned()),
                role: SourceAssetRole::Pdf,
            },
            SourceAsset {
                asset_url: "https://api.govinfo.gov/packages/USREPORTS-99/xml".to_owned(),
                label: "XML".to_owned(),
                mime_type: Some("application/xml".to_owned()),
                role: SourceAssetRole::Other,
            },
        ],
        text_preview: None,
        citation_note: Some(
            "GovInfo publication metadata. Verify package/granule links and cited pages in the official publication."
                .to_owned(),
        ),
        terms_note: Some("Use official GovInfo API search/package/granule endpoints and prefer PDF/XML/MODS links over HTML scraping.".to_owned()),
    }
}

fn enqueue_govinfo_job(store: &mut SqliteStore) {
    store
        .create_ingestion_job(&NewIngestionJob {
            job_key: "ingest:govinfo:USREPORTS-99".to_owned(),
            operation: "ingest".to_owned(),
            source: "govinfo".to_owned(),
            source_id: Some("USREPORTS-99".to_owned()),
            target_url: None,
            next_action: "queued".to_owned(),
        })
        .expect("create GovInfo ingestion job");
}

#[tokio::test]
async fn govinfo_executor_loopback_persists_metadata_and_plan_contract() {
    let loopback_pdf_url = fixture_http_url(b"%PDF govinfo fixture");
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    let files_dir = tempfile::tempdir().expect("create files tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    enqueue_govinfo_job(&mut store);

    let executor = QueuedIngestionExecutor::new(
        "govinfo-worker",
        vec![Arc::new(FakeGovInfoAdapter {
            record: govinfo_record(loopback_pdf_url.clone()),
        })],
    )
    .expect("build GovInfo executor")
    .with_chunk_options(ChunkOptions { target_tokens: 4 });

    let (returned_store, run_result) = executor.run_next(store, &files, &FixturePdfExtractor).await;
    store = returned_store;
    let outcome = run_result
        .expect("GovInfo job execution should succeed")
        .expect("GovInfo job should be claimed");

    assert_eq!(outcome.document_key, "govinfo_usreports_99");
    assert_eq!(outcome.page_count, 2);
    assert_eq!(outcome.chunk_count, 2);
    assert_eq!(
        outcome.warnings,
        vec!["govinfo fixture extractor warning".to_owned()]
    );

    let job = store
        .get_ingestion_job_record("ingest:govinfo:USREPORTS-99")
        .expect("load persisted job state");
    assert_eq!(job.status, "succeeded");
    assert_eq!(job.stage, "succeeded");
    assert_eq!(job.progress, 1.0);
    assert_eq!(
        job.warnings,
        vec!["govinfo fixture extractor warning".to_owned()]
    );

    let document = store
        .get_document_metadata("govinfo:USREPORTS-99")
        .expect("load persisted document metadata");
    assert_eq!(document.source, "govinfo");
    assert_eq!(document.source_id, "USREPORTS-99");
    assert_eq!(document.page_count, 2);
    assert_eq!(
        document.origin_url.as_deref(),
        Some("https://www.govinfo.gov/app/details/USREPORTS-99")
    );
    assert_eq!(
        document.document_url.as_deref(),
        Some("https://api.govinfo.gov/packages/USREPORTS-99/summary")
    );
    assert_eq!(document.pdf_url.as_deref(), Some(loopback_pdf_url.as_str()));
    assert_eq!(
        document.citation_note.as_deref(),
        Some("GovInfo publication metadata. Verify package/granule links and cited pages in the official publication.")
    );
    assert_eq!(
        document.terms_note.as_deref(),
        Some("Use official GovInfo API search/package/granule endpoints and prefer PDF/XML/MODS links over HTML scraping.")
    );

    let metadata: Value =
        serde_json::from_str(&document.metadata_json).expect("document metadata_json is valid");
    assert_eq!(
        metadata["source_metadata"]["source_warning"],
        GOVINFO_SOURCE_WARNING
    );
    assert_eq!(
        metadata["source_metadata"]["cache_policy_note"],
        GOVINFO_CACHE_POLICY_NOTE
    );
    assert_eq!(
        metadata["source_metadata"]["redirect_policy_note"],
        GOVINFO_REDIRECT_POLICY_NOTE
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
        .get_page_text("govinfo:USREPORTS-99", 1, 2)
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
            ["govinfo:USREPORTS-99"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load persisted asset row");
    assert_eq!(asset_url, loopback_pdf_url);
    assert_eq!(role, "pdf");
    assert_eq!(cache_policy, "respect_source_headers");
}

fn fixture_http_url(body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let address = listener.local_addr().expect("fixture server address");
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
    format!("http://{address}/govinfo-fixture.pdf")
}

fn read_http_request(stream: &mut TcpStream) {
    let mut buffer = [0_u8; 1024];
    let _ = stream.read(&mut buffer);
}
