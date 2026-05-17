use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    fbi_vault::FbiVaultAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_returns_vault_records_with_official_provenance() {
    let body = include_str!("fixtures/fbi_vault/search_results.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = FbiVaultAdapter::new(base_url.clone());
    let page = adapter
        .search("mark page", SearchOptions::default())
        .await
        .expect("search should parse fixture page");

    assert_eq!(page.source, "fbi_vault_search");
    assert_eq!(page.records.len(), 1);
    let record = &page.records[0];
    assert_eq!(record.source_id, "rosenberg-case/mark-page");
    assert_eq!(record.collection.as_deref(), Some("Rosenberg Case"));
    assert_eq!(
        record.metadata.get("vault_slug").map(String::as_str),
        Some("rosenberg-case/mark-page")
    );
    assert_eq!(
        record.metadata.get("listing_origin").map(String::as_str),
        Some("search")
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("official FBI Vault page and PDF URL"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /search?SearchableText=mark+page HTTP/1.1"));
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let body = include_str!("fixtures/fbi_vault/search_results.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = FbiVaultAdapter::new(base_url);
    let page = adapter
        .search("antarctica", SearchOptions::default())
        .await
        .expect("empty search should return warning page");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
    assert!(page.warnings[0].contains("no matching records"));
}

#[tokio::test]
async fn get_record_by_prefixed_slug_resolves_multipart_pdf_assets() {
    let body = include_str!("fixtures/fbi_vault/mark_page.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = FbiVaultAdapter::new(base_url.clone());
    let record = adapter
        .get_record("fbi_vault:rosenberg-case/mark-page")
        .await
        .expect("source id should resolve to official detail page");

    assert_eq!(record.source_id, "rosenberg-case/mark-page");
    assert_eq!(record.attachments.len(), 4);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[2].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[3].role, SourceAssetRole::Image);
    assert_eq!(
        record.attachments[0].asset_url,
        format!("{base_url}/rosenberg-case/mark-page/Mark%20Page%20Part%2001/at_download/file")
    );
    assert_eq!(
        record.metadata.get("pdf_asset_count").map(String::as_str),
        Some("3")
    );
    assert_eq!(
        record.metadata.get("part_count").map(String::as_str),
        Some("3")
    );
    assert_eq!(
        record
            .metadata
            .get("primary_asset_label")
            .map(String::as_str),
        Some("Mark Page Part 01")
    );
    let expected_page_url = format!("{base_url}/rosenberg-case/mark-page");
    assert_eq!(
        record.metadata.get("official_page_url").map(String::as_str),
        Some(expected_page_url.as_str())
    );
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("historically uneven multipart layouts"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /rosenberg-case/mark-page HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_official_vault_url() {
    let body = include_str!("fixtures/fbi_vault/mark_page.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let record_url = format!("{base_url}/rosenberg-case/mark-page");
    let adapter = FbiVaultAdapter::new(base_url);
    let record = adapter
        .get_record(&record_url)
        .await
        .expect("official URL should resolve");

    assert_eq!(record.source_id, "rosenberg-case/mark-page");
    let expected_pdf = format!("{record_url}/Mark%20Page%20Part%2001/at_download/file");
    assert_eq!(record.pdf_url.as_deref(), Some(expected_pdf.as_str()));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /rosenberg-case/mark-page HTTP/1.1"));
}

#[tokio::test]
async fn get_record_direct_download_url_returns_single_pdf_asset() {
    let adapter = FbiVaultAdapter::default();

    let record = adapter
        .get_record(
            "https://vault.fbi.gov/rosenberg-case/mark-page/Mark%20Page%20Part%2001/at_download/file",
        )
        .await
        .expect("direct at_download URL should map without extra HTTP");

    assert_eq!(
        record.source_id,
        "rosenberg-case/mark-page/Mark%20Page%20Part%2001"
    );
    assert_eq!(record.attachments.len(), 1);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        record.pdf_url.as_deref(),
        Some(
            "https://vault.fbi.gov/rosenberg-case/mark-page/Mark%20Page%20Part%2001/at_download/file"
        )
    );
}

#[tokio::test]
async fn list_assets_orders_pdf_parts_naturally_before_non_pdf() {
    let body = include_str!("fixtures/fbi_vault/mark_page.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = FbiVaultAdapter::new(base_url.clone());
    let record = adapter
        .get_record("rosenberg-case/mark-page")
        .await
        .expect("record should resolve");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should sort/dedupe");

    assert_eq!(assets.len(), 4);
    assert_eq!(assets[0].label, "Mark Page Part 01");
    assert_eq!(assets[1].label, "Mark Page Part 02");
    assert_eq!(assets[2].label, "Mark Page Part 10 (Final)");
    assert_eq!(assets[3].role, SourceAssetRole::Image);
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = FbiVaultAdapter::default();

    let err = adapter
        .get_record("https://example.com/not-vault")
        .await
        .expect_err("non-official URL should fail");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official vault.fbi.gov URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/fbi_vault/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = FbiVaultAdapter::new(base_url);
    let err = adapter
        .get_record("rosenberg-case/mark-page")
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

    let adapter = FbiVaultAdapter::new(base_url);
    let err = adapter
        .search("ufo", SearchOptions::default())
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
