use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    noaa::NoaaAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_parses_noaa_records_and_dedupes_duplicate_repository_ids() {
    let body = include_str!("fixtures/noaa/search_success.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let page = adapter
        .search(
            "coastal temperature",
            SearchOptions {
                max_results: 10,
                cursor: Some("cursor-1".to_owned()),
            },
        )
        .await
        .expect("search should parse NOAA fixture");

    assert_eq!(page.source, "noaa_repository_search");
    assert_eq!(page.records.len(), 1);

    let record = &page.records[0];
    assert_eq!(record.id, "noaa:72421");
    assert_eq!(record.source_id, "72421");
    assert_eq!(
        record.document_url,
        "https://repository.library.noaa.gov/view/noaa/72421"
    );
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://repository.library.noaa.gov/view/noaa/72421/noaa_72421_DS1.pdf")
    );
    assert_eq!(
        record
            .metadata
            .get("noaa_office_program")
            .map(String::as_str),
        Some("Atlantic Oceanographic and Meteorological Laboratory")
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("official repository item URL"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("GET /search?query=coastal%20temperature&start=cursor-1 HTTP/1.1")
    );
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let body = include_str!("fixtures/noaa/search_success.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let page = adapter
        .search("antarctica sea-ice budget", SearchOptions::default())
        .await
        .expect("search should return warning page");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
    assert!(page.warnings[0].contains("returned no matching records"));
}

#[tokio::test]
async fn search_empty_fixture_returns_warning() {
    let body = include_str!("fixtures/noaa/search_empty.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let page = adapter
        .search("stormfury", SearchOptions::default())
        .await
        .expect("empty fixture should parse");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
}

#[tokio::test]
async fn get_record_accepts_noaa_prefixed_id_and_parses_detail_metadata() {
    let body = include_str!("fixtures/noaa/detail_record.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let record = adapter
        .get_record("noaa:72421")
        .await
        .expect("record should parse detail fixture");

    assert_eq!(record.id, "noaa:72421");
    assert_eq!(record.source_id, "72421");
    assert_eq!(
        record.metadata.get("report_number").map(String::as_str),
        Some("EDM-2024-09")
    );
    assert_eq!(
        record.metadata.get("doi").map(String::as_str),
        Some("10.25923/tcjt-3a69")
    );
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /view/noaa/72421 HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_plain_id_and_official_url() {
    let body = include_str!("fixtures/noaa/detail_record.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body), response_html(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let by_plain = adapter
        .get_record("72421")
        .await
        .expect("plain id should resolve");
    let by_official = adapter
        .get_record("https://repository.library.noaa.gov/view/noaa/72421")
        .await
        .expect("official url should resolve");

    assert_eq!(by_plain.source_id, "72421");
    assert_eq!(by_official.source_id, "72421");

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /view/noaa/72421 HTTP/1.1"));
    assert!(requests[1].starts_with("GET /view/noaa/72421 HTTP/1.1"));
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = NoaaAdapter::new("http://127.0.0.1:9");

    let err = adapter
        .get_record("https://example.com/view/noaa/72421")
        .await
        .expect_err("non-official URL should fail before HTTP");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official repository.library.noaa.gov URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/noaa/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let err = adapter
        .search("weather", SearchOptions::default())
        .await
        .expect_err("invalid HTML should fail as source-changed");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("search-results container"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn json_body_returns_source_changed() {
    let body = include_str!("fixtures/noaa/invalid_json_response.json");
    let (base_url, _requests) = serve_sequence(vec![response_json(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let err = adapter
        .search("weather", SearchOptions::default())
        .await
        .expect_err("json body should fail for html parser endpoint");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("returned JSON"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn xml_body_returns_source_changed() {
    let body = include_str!("fixtures/noaa/invalid_xml_response.xml");
    let (base_url, _requests) = serve_sequence(vec![response_xml(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let err = adapter
        .get_record("72421")
        .await
        .expect_err("xml body should fail for html parser endpoint");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("returned XML"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn redirect_response_is_denied_for_search_fetch() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    )]);
    let adapter = NoaaAdapter::new(base_url);

    let err = adapter
        .search("stormfury", SearchOptions::default())
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

#[tokio::test]
async fn list_assets_orders_pdf_first_and_dedupes() {
    let body = include_str!("fixtures/noaa/detail_record.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let adapter = NoaaAdapter::new(base_url);

    let record = adapter
        .get_record("72421")
        .await
        .expect("detail record should parse");

    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should return sorted assets");

    assert!(!assets.is_empty());
    assert_eq!(assets[0].role, SourceAssetRole::Pdf);
    assert!(assets
        .iter()
        .any(|asset| asset.role == SourceAssetRole::Html));

    let pdf_count = assets
        .iter()
        .filter(|asset| asset.role == SourceAssetRole::Pdf)
        .count();
    assert_eq!(pdf_count, 1);
    assert!(assets.iter().any(|asset| {
        asset.role == SourceAssetRole::Other
            && asset.asset_url == "https://www.noaa.gov/example/source-report.pdf"
    }));
    assert!(!assets.iter().any(|asset| {
        asset.role == SourceAssetRole::Pdf
            && asset.asset_url == "https://www.noaa.gov/example/source-report.pdf"
    }));
}

fn response_html(body: &'static str) -> ResponseSpec {
    (
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )
}

fn response_json(body: &'static str) -> ResponseSpec {
    (
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "application/json")],
        body,
    )
}

fn response_xml(body: &'static str) -> ResponseSpec {
    (
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "application/xml")],
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
