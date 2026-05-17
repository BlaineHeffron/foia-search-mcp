use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    nara::{make_cursor, NaraAdapter},
    CachePolicy, SearchOptions, SourceAdapter, SourceError,
};

#[tokio::test]
async fn missing_key_returns_actionable_error_without_http() {
    let adapter = NaraAdapter::new("http://127.0.0.1:9/api/v2", None);

    let err = adapter
        .search("weather", SearchOptions::default())
        .await
        .expect_err("missing key should fail before HTTP");

    match err {
        SourceError::InvalidInput {
            message, guidance, ..
        } => {
            assert!(message.contains("FOIA_SEARCH_NARA_API_KEY"));
            assert!(guidance
                .as_deref()
                .unwrap_or_default()
                .contains("Set FOIA_SEARCH_NARA_API_KEY"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn search_fetches_json_with_api_key_and_normalizes_records() {
    let body = include_str!("fixtures/nara/search_success.json");
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", "application/json", body);
    let adapter = NaraAdapter::new(base_url, Some("test-key".to_owned()));

    let page = adapter
        .search(
            "weather modification",
            SearchOptions {
                max_results: 1,
                cursor: Some(make_cursor(10)),
            },
        )
        .await
        .expect("local NARA search response should parse");

    assert_eq!(page.source, "nara_catalog");
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].id, "nara:595353");
    assert_eq!(page.records[0].source_id, "595353");
    assert_ne!(page.records[0].document_key, page.records[0].source_id);
    assert_eq!(
        page.records[0].pdf_url.as_deref(),
        Some("https://catalog.archives.gov/files/595353/report.pdf")
    );
    assert_eq!(page.records[0].attachments.len(), 2);
    assert_eq!(page.next_cursor.as_deref(), Some("nara-offset-11"));
    assert_eq!(adapter.cache_policy(), CachePolicy::DoNotPersist);

    let request = request.join().expect("request thread should finish");
    assert!(request.starts_with(
        "GET /api/v2/records/search?q=weather+modification&limit=1&page=11&availableOnline=true "
    ));
    assert!(request.contains("\r\nx-api-key: test-key\r\n"));
    assert!(request.contains("\r\naccept: application/json\r\n"));
}

#[tokio::test]
async fn get_record_fetches_detail_by_naid_url() {
    let body = include_str!("fixtures/nara/detail_success.json");
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", "application/json", body);
    let adapter = NaraAdapter::new(base_url, Some("test-key".to_owned()));

    let record = adapter
        .get_record("https://catalog.archives.gov/id/595353")
        .await
        .expect("local NARA detail response should parse");

    assert_eq!(record.title, "Weather Modification Report");
    assert_eq!(record.source_id, "595353");
    assert_eq!(
        record.document_url,
        "https://catalog.archives.gov/id/595353"
    );
    assert!(record
        .text_preview
        .as_deref()
        .unwrap_or_default()
        .contains("Detailed scope text"));
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("Persistent API response caching is disabled"));

    let request = request.join().expect("request thread should finish");
    assert!(request.starts_with("GET /api/v2/records/search?naId=595353&limit=1 "));
    assert!(request.contains("\r\nx-api-key: test-key\r\n"));
}

#[tokio::test]
async fn get_record_returns_error_when_naid_is_missing() {
    let (base_url, request) = serve_once(
        "HTTP/1.1 200 OK",
        "application/json",
        r#"{"body":{"hits":{"total":0,"records":[]}}}"#,
    );
    let adapter = NaraAdapter::new(base_url, Some("test-key".to_owned()));

    let err = adapter
        .get_record("595353")
        .await
        .expect_err("empty NARA detail response should not fabricate a record");

    match err {
        SourceError::Fetch { message, .. } => {
            assert!(message.contains("no Catalog record for NAID 595353"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("GET /api/v2/records/search?naId=595353&limit=1 "));
}

#[tokio::test]
async fn html_response_returns_source_changed_error() {
    let (base_url, request) = serve_once(
        "HTTP/1.1 200 OK",
        "text/html; charset=utf-8",
        "<html><body>not json</body></html>",
    );
    let adapter = NaraAdapter::new(base_url, Some("test-key".to_owned()));

    let err = adapter
        .search("weather", SearchOptions::default())
        .await
        .expect_err("HTML should not be parsed as JSON");

    match err {
        SourceError::SourceChanged {
            message,
            url: Some(url),
            ..
        } => {
            assert!(message.contains("returned HTML instead of JSON"));
            assert!(url.contains("/api/v2/records/search"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("GET /api/v2/records/search?q=weather"));
}

#[tokio::test]
async fn redirect_response_is_denied_without_following_target() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, request) = serve_once_with_headers(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    );
    let adapter = NaraAdapter::new(base_url, Some("test-key".to_owned()));

    let err = adapter
        .search("weather", SearchOptions::default())
        .await
        .expect_err("redirect should be denied before following target");

    match err {
        SourceError::Fetch {
            message,
            url: Some(url),
            ..
        } => {
            assert!(message.contains("redirect HTTP 302"));
            assert!(message.contains("denied by default"));
            assert!(message.contains(redirect_target));
            assert!(url.contains("/api/v2/records/search?q=weather"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("GET /api/v2/records/search?q=weather"));
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
        let mut buffer = [0; 4096];
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

    (format!("http://{addr}/api/v2"), handle)
}
