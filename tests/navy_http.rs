use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    navy::NavyAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_returns_official_navy_reading_room_leads() {
    let reading_room = include_str!("fixtures/navy/reading_room.html");
    let audit = include_str!("fixtures/navy/audit_room.html");
    let ig = include_str!("fixtures/navy/ig_room.html");
    let (base_url, requests) = serve_sequence(vec![
        response_html(reading_room),
        response_html(audit),
        response_html(ig),
    ]);

    let adapter = NavyAdapter::new(base_url.clone());
    let page = adapter
        .search("Scorpion", SearchOptions::default())
        .await
        .expect("search should parse Navy fixtures");

    assert_eq!(page.source, "navy_foia_reading_room");
    assert_eq!(page.records.len(), 1);
    let record = &page.records[0];
    assert_eq!(
        record.source_id,
        "foia/readingroom/CaseFiles/Scorpion%20Submarine/Rule%20Letter%20to%20CSF%20of%2030JUN09.pdf"
    );
    assert_eq!(
        record.collection.as_deref(),
        Some("Department of the Navy FOIA Reading Room")
    );
    assert_eq!(record.date.as_deref(), Some("2018-04-23"));
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        record.pdf_url.as_deref(),
        Some(record.document_url.as_str())
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("Official Department of the Navy FOIA Reading Room"));
    assert!(record
        .metadata
        .get("source_warning")
        .map(String::as_str)
        .unwrap_or_default()
        .contains("Page-level citations"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /foia/readingroom/SitePages/Home.aspx HTTP/1.1"));
    assert!(requests[1].starts_with("GET /navaudsvc/foia-reading-room HTTP/1.1"));
    assert!(requests[2].starts_with("GET /ig/Pages/foia2.aspx HTTP/1.1"));
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let reading_room = include_str!("fixtures/navy/reading_room.html");
    let audit = include_str!("fixtures/navy/audit_room.html");
    let ig = include_str!("fixtures/navy/ig_room.html");
    let (base_url, _requests) = serve_sequence(vec![
        response_html(reading_room),
        response_html(audit),
        response_html(ig),
    ]);

    let adapter = NavyAdapter::new(base_url);
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
async fn get_record_by_returned_id_resolves_official_pdf_url() {
    let adapter = NavyAdapter::default();

    let record = adapter
        .get_record("navy:foia/readingroom/HotTopics/RED HILL INVESTIGATION/RH SLOC_signed.PDF")
        .await
        .expect("returned Navy source id should resolve");

    assert_eq!(
        record.document_url,
        "https://www.secnav.navy.mil/foia/readingroom/HotTopics/RED%20HILL%20INVESTIGATION/RH%20SLOC_signed.PDF"
    );
    assert_eq!(record.attachments.len(), 1);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("Avoid mirrors"));
}

#[tokio::test]
async fn get_record_accepts_official_component_url() {
    let ig = include_str!("fixtures/navy/ig_room.html");
    let (base_url, requests) = serve_sequence(vec![response_html(ig)]);
    let record_url = format!("{base_url}/ig/Pages/foia2.aspx");

    let adapter = NavyAdapter::new(base_url);
    let record = adapter
        .get_record(&record_url)
        .await
        .expect("official Navy component URL should resolve");

    assert_eq!(record.source_id, "ig/Pages/foia2.aspx");
    assert_eq!(
        record.collection.as_deref(),
        Some("Naval Inspector General FOIA Reading Room")
    );
    assert_eq!(record.attachments.len(), 2);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::OcrText);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /ig/Pages/foia2.aspx HTTP/1.1"));
}

#[tokio::test]
async fn list_assets_prefers_pdfs_before_non_pdf_assets() {
    let ig = include_str!("fixtures/navy/ig_room.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(ig)]);

    let adapter = NavyAdapter::new(base_url);
    let record = adapter
        .get_record("ig/Pages/foia2.aspx")
        .await
        .expect("record should resolve");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should sort/dedupe assets");

    assert_eq!(assets.len(), 2);
    assert_eq!(assets[0].role, SourceAssetRole::Pdf);
    assert_eq!(assets[1].role, SourceAssetRole::OcrText);
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = NavyAdapter::default();

    let err = adapter
        .get_record("https://example.com/not-navy")
        .await
        .expect_err("non-official URL should fail");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official same-origin secnav.navy.mil URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn rejects_official_url_outside_documented_family_scope() {
    let adapter = NavyAdapter::default();

    let err = adapter
        .get_record("https://www.secnav.navy.mil/unrelated/report.pdf")
        .await
        .expect_err("official but out-of-scope URL should fail");

    match err {
        SourceError::InvalidInput { guidance, .. } => {
            assert!(guidance
                .as_deref()
                .unwrap_or_default()
                .contains("/foia, /ig, or /navaudsvc"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/navy/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = NavyAdapter::new(base_url);
    let err = adapter
        .get_record("ig/Pages/foia2.aspx")
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

    let adapter = NavyAdapter::new(base_url);
    let err = adapter
        .search("Scorpion", SearchOptions::default())
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
