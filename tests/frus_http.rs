use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    frus::FrusAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_catalog_parses_frus_records_and_metadata() {
    let body = include_str!("fixtures/frus/catalog_search_success.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);
    let adapter = FrusAdapter::new(base_url);

    let page = adapter
        .search(
            "SALT Kissinger",
            SearchOptions {
                max_results: 10,
                cursor: Some("cursor-abc".to_owned()),
            },
        )
        .await
        .expect("search should parse fixture");

    assert_eq!(page.source, "history_state_gov_frus");
    assert_eq!(page.records.len(), 1);
    let record = &page.records[0];
    assert_eq!(record.id, "frus:frus1969-76v12/d34");
    assert_eq!(record.source_id, "frus1969-76v12/d34");
    assert_eq!(
        record.metadata.get("document_number").map(String::as_str),
        Some("34")
    );
    assert_eq!(
        record.metadata.get("volume_title").map(String::as_str),
        Some("1969-1976, Volume XII, Soviet Union, January 1969-October 1970")
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("canonical history.state.gov document URL"));

    let requests = requests.join().expect("requests should capture");
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .starts_with("GET /search?within=documents&q=SALT%20Kissinger&start=cursor-abc HTTP/1.1"));
}

#[tokio::test]
async fn search_empty_returns_warning() {
    let body = include_str!("fixtures/frus/catalog_search_empty.html");
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);
    let adapter = FrusAdapter::new(base_url);

    let page = adapter
        .search("narrow topic", SearchOptions::default())
        .await
        .expect("empty should still parse");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
    assert!(page.warnings[0].contains("no matching records"));
}

#[tokio::test]
async fn get_record_accepts_frus_prefixed_id() {
    let body = include_str!("fixtures/frus/detail_record.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);
    let adapter = FrusAdapter::new(base_url);

    let record = adapter
        .get_record("frus:frus1969-76v12/d34")
        .await
        .expect("frus id should resolve");

    assert_eq!(record.id, "frus:frus1969-76v12/d34");
    assert_eq!(
        record.metadata.get("volume_id").map(String::as_str),
        Some("frus1969-76v12")
    );
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://static.history.state.gov/frus/frus1969-76v12/pdf/frus1969-76v12.pdf")
    );
    assert_eq!(
        record.metadata.get("persons").map(String::as_str),
        Some("Gromyko | Kissinger")
    );
    assert_eq!(
        record.metadata.get("places").map(String::as_str),
        Some("Moscow")
    );

    let requests = requests.join().expect("requests should capture");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /historicaldocuments/frus1969-76v12/d34 HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_official_history_state_url() {
    let body = include_str!("fixtures/frus/detail_record.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);
    let adapter = FrusAdapter::new(base_url);

    let record = adapter
        .get_record("https://history.state.gov/historicaldocuments/frus1969-76v12/d34")
        .await
        .expect("official URL should resolve");

    assert_eq!(record.source_id, "frus1969-76v12/d34");
    assert_eq!(
        record.document_url,
        "https://history.state.gov/historicaldocuments/frus1969-76v12/d34"
    );

    let requests = requests.join().expect("requests should capture");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /historicaldocuments/frus1969-76v12/d34 HTTP/1.1"));
}

#[tokio::test]
async fn non_official_url_is_rejected_before_http() {
    let adapter = FrusAdapter::new("http://127.0.0.1:9");

    let err = adapter
        .get_record("https://example.com/historicaldocuments/frus1969-76v12/d34")
        .await
        .expect_err("non-official URL should fail before HTTP");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official history.state.gov URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn unexpected_search_html_returns_source_changed() {
    let body = include_str!("fixtures/frus/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);
    let adapter = FrusAdapter::new(base_url);

    let err = adapter
        .search("SALT", SearchOptions::default())
        .await
        .expect_err("unexpected html should fail for search parse");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("unexpected non-search HTML"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn catalog_shape_change_returns_source_changed() {
    let body = include_str!("fixtures/frus/catalog_search_shape_changed.html");
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);
    let adapter = FrusAdapter::new(base_url);

    let err = adapter
        .search("SALT", SearchOptions::default())
        .await
        .expect_err("shape change should fail");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("unexpected non-search HTML"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn detail_shape_change_returns_source_changed() {
    let body = include_str!("fixtures/frus/detail_shape_changed.html");
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);
    let adapter = FrusAdapter::new(base_url);

    let err = adapter
        .get_record("frus:frus1969-76v12/d34")
        .await
        .expect_err("shape changed html should fail");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("FRUS catalog/detail format may have changed"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn redirect_response_is_denied_for_catalog_search() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    )]);
    let adapter = FrusAdapter::new(base_url);

    let err = adapter
        .search("SALT", SearchOptions::default())
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
async fn list_assets_prefers_tei_pdf_html_then_epub() {
    let body = include_str!("fixtures/frus/detail_record.html");
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);
    let adapter = FrusAdapter::new(base_url);

    let record = adapter
        .get_record("frus1969-76v12/d34")
        .await
        .expect("record should parse");

    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should return ordered assets");

    assert_eq!(assets.len(), 4);
    assert_eq!(assets[0].role, SourceAssetRole::Transcript);
    assert_eq!(assets[1].role, SourceAssetRole::Pdf);
    assert_eq!(assets[2].role, SourceAssetRole::Html);
    assert_eq!(assets[3].role, SourceAssetRole::Other);
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
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("test server should write");
        }
        requests
    });

    (format!("http://{addr}"), handle)
}
