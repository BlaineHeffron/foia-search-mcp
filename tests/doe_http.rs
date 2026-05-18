use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use foia_search::sources::{
    doe::DoeAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

#[tokio::test]
async fn search_returns_official_doe_opennet_leads() {
    let body = include_str!("fixtures/doe/search_success.html");
    let server = serve_responses(vec![response(200, body)]);
    let adapter = DoeAdapter::new(server.base_url());

    let page = adapter
        .search(
            "plutonium",
            SearchOptions {
                max_results: 3,
                cursor: None,
            },
        )
        .await
        .expect("search should parse DOE OpenNet fixture");

    assert_eq!(page.source, "doe_opennet_search");
    assert_eq!(page.records.len(), 3);
    let record = &page.records[0];
    assert_eq!(record.id, "doe:1824644");
    assert_eq!(record.source_id, "1824644");
    assert_eq!(
        record.document_url,
        "https://www.osti.gov/opennet/detail?osti-id=1824644"
    );
    assert_eq!(
        record.metadata.get("accession_number").map(String::as_str),
        Some("NV0601998")
    );
    assert!(record
        .metadata
        .get("source_warning")
        .is_some_and(|warning| warning.contains("official DOE/OSTI")));
    assert!(record
        .citation_note
        .as_deref()
        .is_some_and(|note| note.contains("page-boundary verification")));

    let requests = server.join();
    assert!(requests[0].starts_with("POST /opennet/search-results?page=1 HTTP/1.1"));
    assert!(requests[0].contains("search-for=plutonium"));
}

#[tokio::test]
async fn search_empty_returns_warning_page() {
    let body = include_str!("fixtures/doe/search_empty.html");
    let server = serve_responses(vec![response(200, body)]);
    let adapter = DoeAdapter::new(server.base_url());

    let page = adapter
        .search("not-a-real-topic", SearchOptions::default())
        .await
        .expect("empty search fixture should parse");

    assert!(page.records.is_empty());
    assert!(page.warnings[0].contains("returned no matching records"));
}

#[tokio::test]
async fn get_record_accepts_prefixed_id_and_lists_pdf_first() {
    let body = include_str!("fixtures/doe/detail_record.html");
    let server = serve_responses(vec![response(200, body)]);
    let adapter = DoeAdapter::new(server.base_url());

    let record = adapter
        .get_record("doe:1824644")
        .await
        .expect("detail record should parse");

    assert_eq!(record.id, "doe:1824644");
    assert_eq!(
        record.title,
        "NNSS SOILS MONITORING: PLUTONIUM VALLEY (CAU 366) FY2019 - PUBLICATION NO. 45293 (CD-ROM)"
    );
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://www.osti.gov/opennet/includes/docs/sample-doe-opennet.pdf")
    );
    assert_eq!(
        record.metadata.get("document_pages").map(String::as_str),
        Some("0078")
    );
    assert_eq!(
        record
            .metadata
            .get("declassification_status")
            .map(String::as_str),
        Some("Never classified")
    );

    let assets = adapter
        .list_assets(&record)
        .await
        .expect("assets should list");
    assert_eq!(assets[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        assets[0].asset_url,
        "https://www.osti.gov/opennet/includes/docs/sample-doe-opennet.pdf"
    );
    assert!(assets
        .iter()
        .any(|asset| asset.role == SourceAssetRole::Html
            && asset.asset_url == "https://www.osti.gov/opennet/detail?osti-id=1824644"));
}

#[tokio::test]
async fn get_record_accepts_official_url_and_rejects_non_official_url() {
    let body = include_str!("fixtures/doe/detail_record.html");
    let server = serve_responses(vec![response(200, body)]);
    let adapter = DoeAdapter::new(server.base_url());

    let record = adapter
        .get_record("https://www.osti.gov/opennet/detail?osti-id=1824644")
        .await
        .expect("official DOE OpenNet URL should resolve");
    assert_eq!(record.id, "doe:1824644");

    let error = adapter
        .get_record("https://example.com/opennet/detail?osti-id=1824644")
        .await
        .expect_err("non-official URL should be rejected");
    match error {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_search_html_reports_source_changed() {
    let body = include_str!("fixtures/doe/invalid_html_response.html");
    let server = serve_responses(vec![response(200, body)]);
    let adapter = DoeAdapter::new(server.base_url());

    let error = adapter
        .search("plutonium", SearchOptions::default())
        .await
        .expect_err("invalid source HTML should fail");

    match error {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("missing the expected search results table"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn search_redirect_is_denied() {
    let server = serve_responses(vec![redirect_response("https://www.osti.gov/opennet/")]);
    let adapter = DoeAdapter::new(server.base_url());

    let error = adapter
        .search("plutonium", SearchOptions::default())
        .await
        .expect_err("redirect should fail");

    match error {
        SourceError::Fetch { message, .. } => {
            assert!(message.contains("Redirect responses are denied"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

struct TestServer {
    addr: std::net::SocketAddr,
    handle: thread::JoinHandle<Vec<String>>,
}

impl TestServer {
    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn join(self) -> Vec<String> {
        self.handle.join().expect("request capture should finish")
    }
}

fn serve_responses(responses: Vec<String>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _peer) = listener.accept().expect("test server should accept");
            let mut buffer = [0_u8; 8192];
            let read = stream.read(&mut buffer).expect("test server should read");
            requests.push(String::from_utf8_lossy(&buffer[..read]).to_string());
            stream
                .write_all(response.as_bytes())
                .expect("test server should write");
        }
        requests
    });
    TestServer { addr, handle }
}

fn response(status: u16, body: &str) -> String {
    let status_text = if status == 200 { "OK" } else { "Error" };
    format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn redirect_response(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}
