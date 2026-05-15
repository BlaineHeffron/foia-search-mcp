use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    cia::{make_cursor, CiaAdapter},
    SearchOptions, SourceAdapter, SourceError,
};

#[tokio::test]
async fn http_search_fetches_parses_and_limits_results() {
    let body = r#"
        <div class="search-result">
          <h3><a href="/readingroom/document/cia-rdp-one">First Result</a></h3>
          <a href="/readingroom/docs/ONE.pdf">PDF</a>
        </div>
        <div class="search-result">
          <h3><a href="/readingroom/document/cia-rdp-two">Second Result</a></h3>
        </div>
        <nav><a rel="next" href="?page=3">Next</a></nav>"#;
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", body);
    let adapter = CiaAdapter::new(base_url);

    let page = adapter
        .search(
            "weather modification",
            SearchOptions {
                max_results: 1,
                cursor: Some(make_cursor(2)),
            },
        )
        .await
        .expect("local search response should parse");

    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].source_id, "cia-rdp-one");
    assert_eq!(page.next_cursor.as_deref(), Some("cia-page-3"));
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("GET /readingroom/search/site/weather%20modification?page=2 "));
}

#[tokio::test]
async fn http_get_record_fetches_and_parses_detail() {
    let body = include_str!("fixtures/cia/document_detail.html");
    let (base_url, request) = serve_once("HTTP/1.1 200 OK", body);
    let adapter = CiaAdapter::new(base_url.clone());

    let record = adapter
        .get_record("cia-rdp-test")
        .await
        .expect("local document response should parse");

    assert_eq!(record.title, "Climate Control");
    assert_eq!(record.source_id, "cia-rdp-test");
    assert!(record
        .document_url
        .starts_with(&format!("{base_url}/readingroom/document/cia-rdp-test")));
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("GET /readingroom/document/cia-rdp-test "));
}

#[tokio::test]
async fn http_failure_returns_actionable_fetch_error() {
    let (base_url, request) = serve_once("HTTP/1.1 503 Service Unavailable", "unavailable");
    let adapter = CiaAdapter::new(base_url);

    let err = adapter
        .search("weather", SearchOptions::default())
        .await
        .expect_err("HTTP 503 should become a fetch error");

    match err {
        SourceError::Fetch {
            message,
            url: Some(url),
            ..
        } => {
            assert!(message.contains("HTTP 503"));
            assert!(message.contains("Retry later"));
            assert!(url.contains("/readingroom/search/site/weather"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(request
        .join()
        .expect("request thread should finish")
        .starts_with("GET /readingroom/search/site/weather "));
}

fn serve_once(
    status_line: &'static str,
    body: &'static str,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let (mut stream, _addr) = listener.accept().expect("test server should accept");
        let mut buffer = [0; 4096];
        let read = stream.read(&mut buffer).expect("test server should read");
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        let response = format!(
            "{status_line}\r\nContent-Length: {}\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("test server should write");
        request
    });

    (format!("http://{addr}"), handle)
}
