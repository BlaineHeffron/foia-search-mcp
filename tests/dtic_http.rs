use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    dtic::DticAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_with_accession_query_fetches_official_record_and_warns_about_fragility() {
    let body = include_str!("fixtures/dtic/detail_record.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);
    let adapter = DticAdapter::new(base_url);

    let page = adapter
        .search("ADA630142 directed energy", SearchOptions::default())
        .await
        .expect("search should resolve accession-driven DTIC lead");

    assert_eq!(page.source, "dtic_public_tracer");
    assert_eq!(page.records.len(), 1);
    assert!(!page.warnings.is_empty());
    assert!(page.warnings[0].contains("not treated as stable APIs"));
    let record = &page.records[0];
    assert_eq!(record.id, "dtic:ADA630142");
    assert_eq!(record.source_id, "ADA630142");
    assert_eq!(
        record.document_url,
        "https://apps.dtic.mil/sti/citations/ADA630142"
    );
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://apps.dtic.mil/sti/pdfs/ADA630142.pdf")
    );
    assert_eq!(
        record.metadata.get("report_number").map(String::as_str),
        Some("AFRL-TR-2016-0012")
    );
    assert_eq!(record.title, "Directed Energy Defense Research Overview");
    assert_eq!(record.date.as_deref(), Some("2016-03-01"));
    assert_eq!(
        record.description.as_deref(),
        Some(
            "This report summarizes military physiological monitoring research and provides publicly releasable findings for defense-health planning."
        )
    );
    assert_eq!(
        record.metadata.get("authors").map(String::as_str),
        Some("Friedl, Karl E.; Smith, Jane")
    );
    assert_eq!(
        record.metadata.get("corporate_author").map(String::as_str),
        Some("Air Force Research Laboratory")
    );
    assert_eq!(
        record.metadata.get("subject_terms").map(String::as_str),
        Some("physiological monitoring; military medicine; warfighter performance")
    );
    assert_eq!(
        record
            .metadata
            .get("distribution_statement")
            .map(String::as_str),
        Some("Approved for public release; distribution unlimited.")
    );
    assert!(record
        .metadata
        .get("source_warning")
        .is_some_and(|warning| warning.contains("fragile")));
    assert!(record
        .citation_note
        .as_deref()
        .is_some_and(|note| note.contains("official DTIC citation page")));
    assert!(record
        .terms_note
        .as_deref()
        .is_some_and(|note| note.contains("distribution/public-release")));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /sti/citations/ADA630142 HTTP/1.1"));
}

#[tokio::test]
async fn search_without_accession_returns_guarded_warning_without_http() {
    let adapter = DticAdapter::new("http://127.0.0.1:9");

    let page = adapter
        .search("stormfury weather", SearchOptions::default())
        .await
        .expect("search should return warning-only page");

    assert!(page.records.is_empty());
    assert!(!page.warnings.is_empty());
    assert!(page.warnings[0].contains("not treated as stable APIs"));
    assert!(page.warnings[1].contains("No DTIC accession id"));
}

#[tokio::test]
async fn get_record_accepts_prefixed_plain_and_official_citation_urls() {
    let body = include_str!("fixtures/dtic/detail_record.html");
    let (base_url, requests) = serve_sequence(vec![
        response_html(body),
        response_html(body),
        response_html(body),
    ]);
    let adapter = DticAdapter::new(base_url);

    let by_prefixed = adapter
        .get_record("dtic:ADA630142")
        .await
        .expect("prefixed accession should resolve");
    let by_plain = adapter
        .get_record("ADA630142")
        .await
        .expect("plain accession should resolve");
    let by_url = adapter
        .get_record("https://apps.dtic.mil/sti/citations/ADA630142")
        .await
        .expect("official citation URL should resolve");

    assert_eq!(by_prefixed.source_id, "ADA630142");
    assert_eq!(by_plain.source_id, "ADA630142");
    assert_eq!(by_url.source_id, "ADA630142");

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /sti/citations/ADA630142 HTTP/1.1"));
    assert!(requests[1].starts_with("GET /sti/citations/ADA630142 HTTP/1.1"));
    assert!(requests[2].starts_with("GET /sti/citations/ADA630142 HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_official_pdf_url_and_resolves_citation_page() {
    let body = include_str!("fixtures/dtic/detail_record.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);
    let adapter = DticAdapter::new(base_url);

    let record = adapter
        .get_record("https://apps.dtic.mil/sti/pdfs/ADA630142.pdf")
        .await
        .expect("official pdf URL should resolve via citation lookup");

    assert_eq!(record.source_id, "ADA630142");
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://apps.dtic.mil/sti/pdfs/ADA630142.pdf")
    );

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /sti/citations/ADA630142 HTTP/1.1"));
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = DticAdapter::new("http://127.0.0.1:9");

    let err = adapter
        .get_record("https://example.com/sti/citations/ADA630142")
        .await
        .expect_err("non-official URL should fail before HTTP");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official https://apps.dtic.mil"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/dtic/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let adapter = DticAdapter::new(base_url);

    let err = adapter
        .get_record("ADA630142")
        .await
        .expect_err("invalid detail should fail as source-changed");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("missing an expected title field"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn json_body_returns_source_changed() {
    let body = include_str!("fixtures/dtic/invalid_json_response.json");
    let (base_url, _requests) = serve_sequence(vec![response_json(body)]);
    let adapter = DticAdapter::new(base_url);

    let err = adapter
        .get_record("ADA630142")
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
async fn redirect_response_is_denied_for_citation_fetch() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    )]);
    let adapter = DticAdapter::new(base_url);

    let err = adapter
        .get_record("ADA630142")
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
async fn list_assets_prefers_pdf_first_and_dedupes() {
    let body = include_str!("fixtures/dtic/detail_record.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let adapter = DticAdapter::new(base_url);

    let record = adapter
        .get_record("ADA630142")
        .await
        .expect("detail record should parse");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("asset listing should sort and dedupe");

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
}

#[tokio::test]
async fn metadata_only_record_does_not_infer_pdf_asset() {
    let body = include_str!("fixtures/dtic/metadata_only_record.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let adapter = DticAdapter::new(base_url);

    let record = adapter
        .get_record("ADA765432")
        .await
        .expect("metadata-only citation page should parse");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("asset listing should remain conservative");

    assert_eq!(record.source_id, "ADA765432");
    assert!(record.pdf_url.is_none());
    assert!(!record.metadata.contains_key("official_pdf_url"));
    assert!(assets
        .iter()
        .all(|asset| asset.role != SourceAssetRole::Pdf));
    assert!(assets
        .iter()
        .any(|asset| asset.asset_url == "https://apps.dtic.mil/sti/citations/ADA765432"));
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
