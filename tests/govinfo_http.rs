use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{govinfo::GovInfoAdapter, SearchOptions, SourceAdapter, SourceError};

#[tokio::test]
async fn search_requires_api_key_before_http() {
    let adapter = GovInfoAdapter::new("http://127.0.0.1:9", None);

    let err = adapter
        .search("hearing", SearchOptions::default())
        .await
        .expect_err("missing key should fail before HTTP");

    match err {
        SourceError::InvalidInput {
            message, guidance, ..
        } => {
            assert!(message.contains("API key"));
            assert!(guidance
                .as_deref()
                .unwrap_or_default()
                .contains("FOIA_SEARCH_GOVINFO_API_KEY"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn search_posts_json_and_normalizes_package_and_granule_results() {
    let body = include_str!("fixtures/govinfo/search_success.json");
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", "application/json", body);
    let adapter = GovInfoAdapter::new(base_url, Some("test-key".to_owned()));

    let page = adapter
        .search(
            "congressional hearing",
            SearchOptions {
                max_results: 2,
                cursor: Some("cursor-123".to_owned()),
            },
        )
        .await
        .expect("local GovInfo search should parse");

    assert_eq!(page.source, "govinfo_search_service");
    assert_eq!(page.records.len(), 2);
    assert_eq!(page.next_cursor.as_deref(), Some("AoE/sample-next-offset"));

    let package = &page.records[0];
    assert_eq!(package.id, "govinfo:USREPORTS-99");
    assert_eq!(package.source_id, "USREPORTS-99");
    assert_eq!(
        package.pdf_url.as_deref(),
        Some("https://api.govinfo.gov/packages/USREPORTS-99/pdf")
    );
    assert_eq!(
        package.document_url,
        "https://api.govinfo.gov/packages/USREPORTS-99/summary"
    );

    let granule = &page.records[1];
    assert_eq!(granule.id, "govinfo:WCPD-2009-01-19/WCPD-2009-01-19-Pg36");
    assert_eq!(granule.source_id, "WCPD-2009-01-19/WCPD-2009-01-19-Pg36");
    assert_eq!(
        granule.pdf_url.as_deref(),
        Some("https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/pdf")
    );
    assert_eq!(
        granule.terms_note.as_deref().unwrap_or_default(),
        "Use official GovInfo API search/package/granule endpoints and prefer PDF/XML/MODS links over HTML scraping."
    );

    let request = request.join().expect("request thread should finish");
    assert!(request.starts_with("POST /search?api_key=test-key HTTP/1.1"));
    assert!(request.contains("\r\ncontent-type: application/json\r\n"));
    assert!(request.contains("\"query\":\"congressional hearing\""));
    assert!(request.contains("\"offsetMark\":\"cursor-123\""));
    assert!(request.contains("\"pageSize\":2"));
}

#[tokio::test]
async fn search_empty_results_returns_warning() {
    let body = include_str!("fixtures/govinfo/search_empty.json");
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", "application/json", body);
    let adapter = GovInfoAdapter::new(base_url, Some("test-key".to_owned()));

    let page = adapter
        .search("very narrow query", SearchOptions::default())
        .await
        .expect("empty response should still parse");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
    assert!(page.warnings[0].contains("returned no records"));
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("POST /search?api_key=test-key HTTP/1.1"));
}

#[tokio::test]
async fn get_record_fetches_package_summary_for_plain_package_id() {
    let body = include_str!("fixtures/govinfo/package_summary.json");
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", "application/json", body);
    let adapter = GovInfoAdapter::new(base_url, Some("test-key".to_owned()));

    let record = adapter
        .get_record("USREPORTS-99")
        .await
        .expect("package summary should parse");

    assert_eq!(record.id, "govinfo:USREPORTS-99");
    assert_eq!(record.source_id, "USREPORTS-99");
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://api.govinfo.gov/packages/USREPORTS-99/pdf")
    );
    assert_eq!(
        record.origin_url,
        "https://www.govinfo.gov/app/details/USREPORTS-99"
    );
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("GET /packages/USREPORTS-99/summary?api_key=test-key HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_details_url_and_fetches_granule_summary() {
    let body = include_str!("fixtures/govinfo/granule_summary.json");
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", "application/json", body);
    let adapter = GovInfoAdapter::new(base_url, Some("test-key".to_owned()));

    let record = adapter
        .get_record("https://www.govinfo.gov/app/details/WCPD-2009-01-19/WCPD-2009-01-19-Pg36")
        .await
        .expect("granule summary should parse");

    assert_eq!(record.id, "govinfo:WCPD-2009-01-19/WCPD-2009-01-19-Pg36");
    assert_eq!(record.source_id, "WCPD-2009-01-19/WCPD-2009-01-19-Pg36");
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/pdf")
    );
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with(
            "GET /packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/summary?api_key=test-key HTTP/1.1"
        ));
}

#[tokio::test]
async fn invalid_html_body_returns_source_changed() {
    let body = include_str!("fixtures/govinfo/invalid_html_response.html");
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", "text/html; charset=utf-8", body);
    let adapter = GovInfoAdapter::new(base_url, Some("test-key".to_owned()));

    let err = adapter
        .get_record("USREPORTS-99")
        .await
        .expect_err("html should be rejected as source changed");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("non-JSON response"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("GET /packages/USREPORTS-99/summary?api_key=test-key HTTP/1.1"));
}

#[tokio::test]
async fn redirect_response_is_denied_for_search_post() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, request) = serve_once_with_headers(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    );
    let adapter = GovInfoAdapter::new(base_url, Some("test-key".to_owned()));

    let err = adapter
        .search("weather", SearchOptions::default())
        .await
        .expect_err("redirect should be denied before following target");

    match err {
        SourceError::Fetch { message, .. } => {
            assert!(message.contains("redirect HTTP 302"));
            assert!(message.contains("denied by default"));
            assert!(message.contains(redirect_target));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("POST /search?api_key=test-key HTTP/1.1"));
}

fn serve_once(
    status_line: &'static str,
    content_type: &'static str,
    body: &'static str,
) -> (String, thread::JoinHandle<String>) {
    serve_once_with_headers(status_line, vec![("Content-Type", content_type)], body)
}

fn serve_once_with_headers(
    status_line: &'static str,
    headers: Vec<(&'static str, &'static str)>,
    body: &'static str,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let (mut stream, _addr) = listener.accept().expect("test server should accept");
        let mut buffer = [0; 8192];
        let read = stream.read(&mut buffer).expect("test server should read");
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
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
        request
    });

    (format!("http://{addr}"), handle)
}
