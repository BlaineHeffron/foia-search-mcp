use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    aaro::AaroAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_reads_uap_records_listing_and_returns_normalized_leads() {
    let body = include_str!("fixtures/aaro/uap_records_listing.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = AaroAdapter::new(base_url.clone());
    let page = adapter
        .search("kona blue", SearchOptions::default())
        .await
        .expect("search should parse listing fixture");

    assert_eq!(page.source, "aaro_uap_records");
    assert_eq!(page.records.len(), 1);

    let record = &page.records[0];
    assert_eq!(
        record.id,
        "aaro:UAP-Records/history-and-origin-of-kona-blue"
    );
    assert_eq!(
        record.source_id,
        "UAP-Records/history-and-origin-of-kona-blue"
    );
    assert_eq!(record.collection.as_deref(), Some("AARO UAP Records"));
    assert_eq!(
        record
            .metadata
            .get("originating_agency")
            .map(String::as_str),
        Some("Department of Homeland Security (DHS)")
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("official AARO page"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /UAP-Records/ HTTP/1.1"));
}

#[tokio::test]
async fn search_returns_warning_when_no_records_match_query() {
    let body = include_str!("fixtures/aaro/uap_records_listing.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = AaroAdapter::new(base_url);
    let page = adapter
        .search("antarctica", SearchOptions::default())
        .await
        .expect("search should return warning page");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
    assert!(page.warnings[0].contains("returned no matching leads"));
}

#[tokio::test]
async fn get_record_by_prefixed_slug_parses_detail_and_metadata_assets() {
    let body = include_str!("fixtures/aaro/kona_blue_detail.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = AaroAdapter::new(base_url.clone());
    let record = adapter
        .get_record("aaro:history-and-origin-of-kona-blue")
        .await
        .expect("prefixed slug should resolve");

    assert_eq!(
        record.source_id,
        "UAP-Records/history-and-origin-of-kona-blue"
    );
    assert_eq!(record.attachments.len(), 5);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[2].role, SourceAssetRole::Image);
    assert_eq!(record.attachments[3].role, SourceAssetRole::Other);
    assert_eq!(record.attachments[4].role, SourceAssetRole::Other);
    assert!(record
        .attachments
        .iter()
        .any(|asset| asset.asset_url.contains("dvidshub.net/video/")));
    assert!(record.attachments.iter().any(|asset| {
        asset.asset_url.contains("archives.gov") && asset.role == SourceAssetRole::Other
    }));
    assert_eq!(
        record
            .metadata
            .get("originating_agency")
            .map(String::as_str),
        Some("Department of Homeland Security (DHS)")
    );
    assert_eq!(
        record.metadata.get("asset_count").map(String::as_str),
        Some("5")
    );
    assert_eq!(
        record.metadata.get("pdf_asset_count").map(String::as_str),
        Some("2")
    );

    let expected_pdf = format!(
        "{base_url}/Portals/136/PDFs/UAP_RECORDS_RESEARCH/History_and_Origin_of_KONA_BLUE_FINAL_508.pdf"
    );
    assert_eq!(record.pdf_url.as_deref(), Some(expected_pdf.as_str()));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /UAP-Records/history-and-origin-of-kona-blue HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_official_aaro_url() {
    let body = include_str!("fixtures/aaro/kona_blue_detail.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let url = format!("{base_url}/UAP-Records/history-and-origin-of-kona-blue");
    let adapter = AaroAdapter::new(base_url);
    let record = adapter
        .get_record(&url)
        .await
        .expect("official AARO URL should resolve");

    assert_eq!(
        record.source_id,
        "UAP-Records/history-and-origin-of-kona-blue"
    );
    assert_eq!(record.document_url, url);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /UAP-Records/history-and-origin-of-kona-blue HTTP/1.1"));
}

#[tokio::test]
async fn list_assets_orders_pdfs_before_images_and_video() {
    let body = include_str!("fixtures/aaro/kona_blue_detail.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = AaroAdapter::new(base_url);
    let record = adapter
        .get_record("aaro:history-and-origin-of-kona-blue")
        .await
        .expect("record should resolve");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should sort assets");

    assert_eq!(assets.len(), 5);
    assert_eq!(assets[0].role, SourceAssetRole::Pdf);
    assert_eq!(assets[1].role, SourceAssetRole::Pdf);
    assert_eq!(assets[2].role, SourceAssetRole::Image);
    assert_eq!(assets[3].role, SourceAssetRole::Other);
    assert_eq!(assets[4].role, SourceAssetRole::Other);
}

#[tokio::test]
async fn search_returns_direct_same_origin_pdf_leads_from_listing() {
    let body = include_str!("fixtures/aaro/uap_records_listing.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = AaroAdapter::new(base_url.clone());
    let page = adapter
        .search("workshop", SearchOptions::default())
        .await
        .expect("search should parse direct PDF lead");

    assert_eq!(page.records.len(), 1);
    let record = &page.records[0];
    assert_eq!(record.source, "aaro");
    assert!(record
        .source_id
        .ends_with("White_Paper_2025_UAP_Workshop.pdf"));
    assert_eq!(record.attachments.len(), 1);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        record.pdf_url.as_deref(),
        Some(record.document_url.as_str())
    );

    let fetched = adapter
        .get_record(&record.id)
        .await
        .expect("returned direct PDF id should resolve without HTTP");
    assert_eq!(fetched.source_id, record.source_id);
    assert_eq!(fetched.pdf_url, record.pdf_url);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = AaroAdapter::default();

    let err = adapter
        .get_record("https://example.com/not-aaro")
        .await
        .expect_err("non-official URL should fail");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official same-origin AARO URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/aaro/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = AaroAdapter::new(base_url);
    let err = adapter
        .get_record("aaro:history-and-origin-of-kona-blue")
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

    let adapter = AaroAdapter::new(base_url);
    let err = adapter
        .search("uap", SearchOptions::default())
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
