use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    army::ArmyAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_returns_official_army_reading_room_leads() {
    let listing = include_str!("fixtures/army/search_results.html");
    let ig = include_str!("fixtures/army/ig_results.html");
    let empty = include_str!("fixtures/army/empty_results.html");
    let (base_url, requests) = serve_sequence(vec![
        response_html(listing),
        response_html(empty),
        response_html(ig),
        response_html(empty),
    ]);

    let adapter = ArmyAdapter::new(base_url.clone());
    let page = adapter
        .search("IG report", SearchOptions::default())
        .await
        .expect("search should parse Army fixtures");

    assert_eq!(page.source, "army_foia_reading_room");
    assert_eq!(page.records.len(), 2);
    let record = &page.records[0];
    assert_eq!(record.source_id, "Home/DocContent/9001");
    assert_eq!(record.collection.as_deref(), Some("Army FOIA Reading Room"));
    assert_eq!(record.date.as_deref(), Some("2024-07-02"));
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        record.pdf_url.as_deref(),
        Some(record.document_url.as_str())
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("Official Army FOIA Reading Room"));
    assert!(record
        .metadata
        .get("source_warning")
        .map(String::as_str)
        .unwrap_or_default()
        .contains("Page-level citations"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("GET / HTTP/1.1"));
    assert!(requests[1].starts_with("GET /Home/publicRecords/78 HTTP/1.1"));
    assert!(requests[2].starts_with("GET /Home/publicRecords/94 HTTP/1.1"));
    assert!(requests[3].starts_with("GET /Home/publicRecords/93 HTTP/1.1"));
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let empty = include_str!("fixtures/army/empty_results.html");
    let (base_url, _requests) = serve_sequence(vec![
        response_html(empty),
        response_html(empty),
        response_html(empty),
        response_html(empty),
    ]);

    let adapter = ArmyAdapter::new(base_url);
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
async fn get_record_by_returned_id_resolves_official_document_url() {
    let adapter = ArmyAdapter::default();

    let record = adapter
        .get_record("army:Home/DocContent/9001")
        .await
        .expect("returned Army source id should resolve");

    assert_eq!(record.source_id, "Home/DocContent/9001");
    assert_eq!(
        record.document_url,
        "https://foia.army.mil/Home/DocContent/9001"
    );
    assert_eq!(record.attachments.len(), 1);
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("Avoid mirrors"));
}

#[tokio::test]
async fn get_record_accepts_official_category_url() {
    let ig = include_str!("fixtures/army/ig_results.html");
    let (base_url, requests) = serve_sequence(vec![response_html(ig)]);
    let record_url = format!("{base_url}/Home/publicRecords/94");

    let adapter = ArmyAdapter::new(base_url);
    let record = adapter
        .get_record(&record_url)
        .await
        .expect("official Army URL should resolve");

    assert_eq!(record.source_id, "Home/publicRecords/94");
    assert_eq!(record.attachments.len(), 2);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::Other);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /Home/publicRecords/94 HTTP/1.1"));
}

#[tokio::test]
async fn list_assets_prefers_pdfs_before_non_pdf_assets() {
    let ig = include_str!("fixtures/army/ig_results.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(ig)]);

    let adapter = ArmyAdapter::new(base_url);
    let record = adapter
        .get_record("Home/publicRecords/94")
        .await
        .expect("record should resolve");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should sort/dedupe assets");

    assert_eq!(assets.len(), 2);
    assert_eq!(assets[0].role, SourceAssetRole::Pdf);
    assert_eq!(assets[1].role, SourceAssetRole::Other);
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = ArmyAdapter::default();

    let err = adapter
        .get_record("https://example.com/not-army")
        .await
        .expect_err("non-official URL should fail");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official same-origin foia.army.mil URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/army/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = ArmyAdapter::new(base_url);
    let err = adapter
        .get_record("Home/publicRecords/94")
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

    let adapter = ArmyAdapter::new(base_url);
    let err = adapter
        .search("IG report", SearchOptions::default())
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

            let mut response = format!(
                "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (name, value) in headers {
                response.push_str(name);
                response.push_str(": ");
                response.push_str(value);
                response.push_str("\r\n");
            }
            response.push_str("\r\n");
            response.push_str(body);
            stream
                .write_all(response.as_bytes())
                .expect("test server should write");
        }
        requests
    });

    (format!("http://{addr}"), handle)
}
