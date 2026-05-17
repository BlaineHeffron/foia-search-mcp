use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    doj_foia::DojFoiaAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_returns_component_records_with_oip_provenance() {
    let body = include_str!("fixtures/doj_foia/oip_components_index.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);
    let index_url = format!("{base_url}/oip/available-documents-all-doj-components");

    let adapter = DojFoiaAdapter::new(index_url);
    let page = adapter
        .search("criminal", SearchOptions::default())
        .await
        .expect("search should parse OIP index fixture");

    assert_eq!(page.source, "doj_component_foia_index");
    assert_eq!(page.records.len(), 1);
    let record = &page.records[0];
    assert_eq!(record.source_id, "criminal-division");
    assert_eq!(
        record.metadata.get("component_name").map(String::as_str),
        Some("Criminal Division")
    );
    assert_eq!(
        record.metadata.get("foia_provenance").map(String::as_str),
        Some("doj_component_proactive_disclosure_index")
    );
    assert_eq!(
        record.metadata.get("lead_origin").map(String::as_str),
        Some("oip_all_components_index")
    );

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /oip/available-documents-all-doj-components HTTP/1.1"));
}

#[tokio::test]
async fn search_includes_official_external_component_link_shape() {
    let body = include_str!("fixtures/doj_foia/oip_components_index.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let index_url = format!("{base_url}/oip/available-documents-all-doj-components");

    let adapter = DojFoiaAdapter::new(index_url);
    let page = adapter
        .search("alcohol", SearchOptions::default())
        .await
        .expect("search should include official external component URL lead");

    assert_eq!(page.records.len(), 1);
    assert_eq!(
        page.records[0].source_id,
        "bureau-of-alcohol-tobacco-firearms-and-explosives"
    );
    assert_eq!(
        page.records[0].document_url,
        "https://www.atf.gov/content/contact-us/FOIA"
    );
}

#[tokio::test]
async fn search_includes_fbi_component_link_shape() {
    let body = include_str!("fixtures/doj_foia/oip_components_index.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let index_url = format!("{base_url}/oip/available-documents-all-doj-components");

    let adapter = DojFoiaAdapter::new(index_url);
    let page = adapter
        .search("federal bureau investigation", SearchOptions::default())
        .await
        .expect("search should include official FBI component URL lead");

    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].source_id, "federal-bureau-of-investigation");
    assert_eq!(page.records[0].document_url, "https://vault.fbi.gov/foia");
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let body = include_str!("fixtures/doj_foia/oip_components_index.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);
    let index_url = format!("{base_url}/oip/available-documents-all-doj-components");

    let adapter = DojFoiaAdapter::new(index_url);
    let page = adapter
        .search("antarctica", SearchOptions::default())
        .await
        .expect("empty search should return warning page");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
    assert!(page.warnings[0].contains("no matching component leads"));
}

#[tokio::test]
async fn get_record_by_source_id_fetches_component_page_and_prefers_pdf_assets() {
    let index_body = include_str!("fixtures/doj_foia/oip_components_index.html");
    let detail_body = include_str!("fixtures/doj_foia/criminal_foia_reading_room.html");
    let (base_url, requests) =
        serve_sequence(vec![response_html(index_body), response_html(detail_body)]);
    let index_url = format!("{base_url}/oip/available-documents-all-doj-components");

    let adapter = DojFoiaAdapter::new(index_url);
    let record = adapter
        .get_record("doj_foia:criminal-division")
        .await
        .expect("source id should resolve through index and component page");

    assert_eq!(record.source_id, "criminal-division");
    assert_eq!(record.record_group.as_deref(), Some("foia_reading_room"));
    assert_eq!(
        record.metadata.get("component_name").map(String::as_str),
        Some("Criminal Division")
    );
    assert_eq!(record.attachments.len(), 3);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[2].role, SourceAssetRole::Other);
    assert!(record
        .pdf_url
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains(".pdf"));
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("official DOJ component page or PDF URL"));
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("avoid bulk scraping"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /oip/available-documents-all-doj-components HTTP/1.1"));
    assert!(requests[1].starts_with("GET /criminal/foia/foia-reading-room-records HTTP/1.1"));
}

#[tokio::test]
async fn get_record_official_url_returns_html_asset_when_no_pdf_available() {
    let body = include_str!("fixtures/doj_foia/civil_foia_library.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = DojFoiaAdapter::new(format!(
        "{base_url}/oip/available-documents-all-doj-components"
    ));
    let record = adapter
        .get_record(&format!("{base_url}/civil/foia-library"))
        .await
        .expect("official component URL should resolve");

    assert_eq!(record.source_id, "civil-division");
    assert_eq!(record.attachments.len(), 1);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Other);
    assert!(record.pdf_url.is_none());
    assert_eq!(
        record.metadata.get("lead_origin").map(String::as_str),
        Some("component_page")
    );

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /civil/foia-library HTTP/1.1"));
}

#[tokio::test]
async fn list_assets_prefers_pdf_then_html_other_roles() {
    let index_body = include_str!("fixtures/doj_foia/oip_components_index.html");
    let detail_body = include_str!("fixtures/doj_foia/criminal_foia_reading_room.html");
    let (base_url, _requests) =
        serve_sequence(vec![response_html(index_body), response_html(detail_body)]);

    let adapter = DojFoiaAdapter::new(format!(
        "{base_url}/oip/available-documents-all-doj-components"
    ));
    let record = adapter
        .get_record("criminal-division")
        .await
        .expect("record should resolve with attachments");
    let assets = adapter
        .list_assets(&record)
        .await
        .expect("list_assets should sort and dedupe");

    assert_eq!(assets.len(), 3);
    assert_eq!(assets[0].role, SourceAssetRole::Pdf);
    assert_eq!(assets[1].role, SourceAssetRole::Pdf);
    assert_eq!(assets[2].role, SourceAssetRole::Other);
}

#[tokio::test]
async fn rejects_non_official_url_before_http() {
    let adapter = DojFoiaAdapter::default();

    let err = adapter
        .get_record("https://example.com/not-doj-foia")
        .await
        .expect_err("non-official URL should fail validation");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official DOJ component FOIA/disclosure URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_component_html_returns_source_changed() {
    let index_body = include_str!("fixtures/doj_foia/oip_components_index.html");
    let invalid_body = include_str!("fixtures/doj_foia/invalid_html_response.html");
    let (base_url, _requests) =
        serve_sequence(vec![response_html(index_body), response_html(invalid_body)]);

    let adapter = DojFoiaAdapter::new(format!(
        "{base_url}/oip/available-documents-all-doj-components"
    ));
    let err = adapter
        .get_record("criminal-division")
        .await
        .expect_err("invalid component page should fail source-changed");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("format may have changed"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn redirect_is_denied_for_oip_index_fetch() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    )]);

    let adapter = DojFoiaAdapter::new(format!(
        "{base_url}/oip/available-documents-all-doj-components"
    ));
    let err = adapter
        .search("criminal", SearchOptions::default())
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
