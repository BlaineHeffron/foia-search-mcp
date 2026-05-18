use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    dia::DiaAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_returns_official_dia_reading_room_leads() {
    let body = include_str!("fixtures/dia/search_results.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = DiaAdapter::new(base_url.clone());
    let page = adapter
        .search("Argentina terrorism", SearchOptions::default())
        .await
        .expect("search should parse DIA fixtures");

    assert_eq!(page.source, "dia_foia_electronic_reading_room");
    assert_eq!(page.records.len(), 1);
    let record = &page.records[0];
    assert_eq!(
        record.source_id,
        "FOIA/FOIA-Electronic-Reading-Room/FileId/162286"
    );
    assert_eq!(
        record.collection.as_deref(),
        Some("DIA FOIA Electronic Reading Room")
    );
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        record.pdf_url.as_deref(),
        Some(record.document_url.as_str())
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("official DIA page"));
    assert!(record
        .metadata
        .get("source_warning")
        .map(String::as_str)
        .unwrap_or_default()
        .contains("Page-level citations"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /FOIA/FOIA-Electronic-Reading-Room/ HTTP/1.1"));
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let body = include_str!("fixtures/dia/empty_results.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = DiaAdapter::new(base_url);
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
    let body = include_str!("fixtures/dia/detail.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = DiaAdapter::new(base_url.clone());
    let record = adapter
        .get_record("dia:FOIA/FOIA-Electronic-Reading-Room/FOIA-Reading-Room-NCCA")
        .await
        .expect("source id should resolve to official detail page");

    assert_eq!(
        record.source_id,
        "FOIA/FOIA-Electronic-Reading-Room/FOIA-Reading-Room-NCCA"
    );
    assert_eq!(record.title, "FOIA Reading Room: NCCA");
    assert_eq!(record.attachments.len(), 3);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[2].role, SourceAssetRole::Html);
    assert_eq!(
        record.metadata.get("pdf_asset_count").map(String::as_str),
        Some("2")
    );
    let expected_pdf = format!("{base_url}/FOIA/FOIA-Electronic-Reading-Room/FileId/170100/");
    assert_eq!(record.pdf_url.as_deref(), Some(expected_pdf.as_str()));
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("Avoid mirrors"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .starts_with("GET /FOIA/FOIA-Electronic-Reading-Room/FOIA-Reading-Room-NCCA HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_official_url() {
    let body = include_str!("fixtures/dia/detail.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);
    let record_url =
        format!("{base_url}/FOIA/FOIA-Electronic-Reading-Room/FOIA-Reading-Room-NCCA/");

    let adapter = DiaAdapter::new(base_url);
    let record = adapter
        .get_record(&record_url)
        .await
        .expect("official DIA URL should resolve");

    assert_eq!(
        record.source_id,
        "FOIA/FOIA-Electronic-Reading-Room/FOIA-Reading-Room-NCCA"
    );
    assert_eq!(record.document_url, record_url);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .starts_with("GET /FOIA/FOIA-Electronic-Reading-Room/FOIA-Reading-Room-NCCA/ HTTP/1.1"));
}

#[tokio::test]
async fn get_record_direct_fileid_url_returns_single_pdf_asset_without_http() {
    let adapter = DiaAdapter::default();

    let record = adapter
        .get_record("https://www.dia.mil/FOIA/FOIA-Electronic-Reading-Room/FileId/162286/")
        .await
        .expect("direct official DIA FileId URL should map without HTTP");

    assert_eq!(record.attachments.len(), 1);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://www.dia.mil/FOIA/FOIA-Electronic-Reading-Room/FileId/162286/")
    );
}

#[tokio::test]
async fn list_assets_prefers_pdfs_before_non_pdf_assets() {
    let body = include_str!("fixtures/dia/detail.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = DiaAdapter::new(base_url);
    let record = adapter
        .get_record("FOIA/FOIA-Electronic-Reading-Room/FOIA-Reading-Room-NCCA")
        .await
        .expect("record should resolve");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should sort/dedupe assets");

    assert_eq!(assets.len(), 3);
    assert_eq!(assets[0].role, SourceAssetRole::Pdf);
    assert_eq!(assets[1].role, SourceAssetRole::Pdf);
    assert_eq!(assets[2].role, SourceAssetRole::Html);
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = DiaAdapter::default();

    let err = adapter
        .get_record("https://example.com/not-dia")
        .await
        .expect_err("non-official URL should fail");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official same-origin www.dia.mil FOIA URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/dia/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = DiaAdapter::new(base_url);
    let err = adapter
        .get_record("FOIA/FOIA-Electronic-Reading-Room/FOIA-Reading-Room-NCCA")
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

    let adapter = DiaAdapter::new(base_url);
    let err = adapter
        .search("Argentina", SearchOptions::default())
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
