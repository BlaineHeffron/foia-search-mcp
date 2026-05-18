use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    state::StateAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_returns_official_state_vrr_leads() {
    let body = include_str!("fixtures/state/search_results.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = StateAdapter::new(base_url.clone());
    let page = adapter
        .search("Kissinger Chile", SearchOptions::default())
        .await
        .expect("search should parse State fixtures");

    assert_eq!(page.source, "state_virtual_reading_room");
    assert_eq!(page.records.len(), 2);
    let record = &page.records[0];
    assert_eq!(
        record.source_id,
        "FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213"
    );
    assert_eq!(
        record.collection.as_deref(),
        Some("State Department Virtual Reading Room")
    );
    assert!(record.attachments.is_empty());
    assert_eq!(
        record.metadata.get("case_number").map(String::as_str),
        Some("F-1990-04213")
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("official State FOIA page"));
    assert!(record
        .metadata
        .get("source_warning")
        .map(String::as_str)
        .unwrap_or_default()
        .contains("OCR"));
    assert_eq!(page.records[1].attachments[0].role, SourceAssetRole::Pdf);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /Search/Results.aspx?searchText=Kissinger+Chile HTTP/1.1"));
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let body = include_str!("fixtures/state/empty_results.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = StateAdapter::new(base_url);
    let page = adapter
        .search("antarctica", SearchOptions::default())
        .await
        .expect("empty search should return warning page");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
    assert!(page.warnings[0].contains("no matching official leads"));
    assert!(!page.warnings[0].contains("fixtures"));
}

#[tokio::test]
async fn get_record_by_prefixed_id_parses_detail_and_orders_assets() {
    let body = include_str!("fixtures/state/detail.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = StateAdapter::new(base_url.clone());
    let record = adapter
        .get_record("state:FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213")
        .await
        .expect("source id should resolve to official detail page");

    assert_eq!(
        record.source_id,
        "FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213"
    );
    assert_eq!(record.title, "Kissinger memorandum on Chile policy");
    assert_eq!(record.attachments.len(), 4);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[2].role, SourceAssetRole::Html);
    assert_eq!(record.attachments[3].role, SourceAssetRole::OcrText);
    assert_eq!(
        record.metadata.get("pdf_asset_count").map(String::as_str),
        Some("2")
    );
    let expected_pdf =
        format!("{base_url}/Documents/F-1990-04213/chile-policy-memorandum-attachment.pdf");
    assert_eq!(record.pdf_url.as_deref(), Some(expected_pdf.as_str()));
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("Avoid mirrors"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .starts_with("GET /FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213 HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_official_url() {
    let body = include_str!("fixtures/state/detail.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);
    let record_url = format!("{base_url}/FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213");

    let adapter = StateAdapter::new(base_url);
    let record = adapter
        .get_record(&record_url)
        .await
        .expect("official State URL should resolve");

    assert_eq!(
        record.source_id,
        "FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213"
    );
    assert_eq!(record.document_url, record_url);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .starts_with("GET /FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213 HTTP/1.1"));
}

#[tokio::test]
async fn get_record_direct_pdf_url_returns_single_pdf_asset_without_http() {
    let adapter = StateAdapter::default();

    let record = adapter
        .get_record("https://foia.state.gov/Documents/F-1990-04213/chile-policy-memorandum.pdf")
        .await
        .expect("direct official State PDF should map without HTTP");

    assert_eq!(record.attachments.len(), 1);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://foia.state.gov/Documents/F-1990-04213/chile-policy-memorandum.pdf")
    );
}

#[tokio::test]
async fn list_assets_prefers_pdfs_before_non_pdf_assets() {
    let body = include_str!("fixtures/state/detail.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = StateAdapter::new(base_url);
    let record = adapter
        .get_record("FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213")
        .await
        .expect("record should resolve");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should sort/dedupe assets");

    assert_eq!(assets.len(), 4);
    assert_eq!(assets[0].role, SourceAssetRole::Pdf);
    assert_eq!(assets[1].role, SourceAssetRole::Pdf);
    assert_eq!(assets[2].role, SourceAssetRole::Html);
    assert_eq!(assets[3].role, SourceAssetRole::OcrText);
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = StateAdapter::default();

    let err = adapter
        .get_record("https://example.com/not-state")
        .await
        .expect_err("non-official URL should fail");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official same-origin foia.state.gov URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/state/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = StateAdapter::new(base_url);
    let err = adapter
        .get_record("FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213")
        .await
        .expect_err("invalid page should fail as source-changed");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("format may have changed"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn redirect_is_denied_for_search_fetch() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    )]);

    let adapter = StateAdapter::new(base_url);
    let err = adapter
        .search("Kissinger", SearchOptions::default())
        .await
        .expect_err("redirect should be denied");

    match err {
        SourceError::Fetch { message, .. } => {
            assert!(message.contains("redirect HTTP 302"));
            assert!(message.contains("denied by default"));
            assert!(message.contains(redirect_target));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

fn response_html(body: &'static str) -> ResponseSpec {
    (
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )
}

fn serve_sequence(responses: Vec<ResponseSpec>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener.local_addr().expect("test server address");

    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status_line, headers, body) in responses {
            let (mut stream, _peer) = listener.accept().expect("test server should accept");
            let mut buffer = [0_u8; 8192];
            let read = stream.read(&mut buffer).expect("test server should read");
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
                body.len(),
            );
            stream
                .write_all(response.as_bytes())
                .expect("test server should write");
        }
        requests
    });

    (format!("http://{addr}"), handle)
}
