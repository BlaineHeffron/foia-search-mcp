use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    doj_epstein::DojEpsteinAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_returns_doj_leads_with_sensitive_warning_and_category_provenance() {
    let body = include_str!("fixtures/doj_epstein/doj_disclosures.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let adapter = DojEpsteinAdapter::new(format!("{base_url}/epstein"));
    let page = adapter
        .search("foia cbp", SearchOptions::default())
        .await
        .expect("search should parse fixture disclosures page");

    assert_eq!(page.source, "doj_epstein_library");
    assert_eq!(page.records.len(), 1);
    assert_eq!(
        page.records[0].source_id,
        "foia-customs-and-border-protection-cbp"
    );
    assert_eq!(
        page.records[0].metadata.get("category").map(String::as_str),
        Some("foia")
    );
    assert_eq!(
        page.records[0]
            .metadata
            .get("component")
            .map(String::as_str),
        Some("Customs and Border Protection (CBP)")
    );
    assert!(page
        .warnings
        .iter()
        .any(|warning| warning.contains("sensitive")));
    assert_sensitive_warning_mentions_privacy_and_victims(
        page.records[0]
            .metadata
            .get("source_warning")
            .expect("record should carry sensitive source warning"),
    );

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /epstein/doj-disclosures HTTP/1.1"));
}

#[tokio::test]
async fn search_preserves_direct_media_ids_for_get_record_lookup() {
    let body = include_str!("fixtures/doj_epstein/doj_disclosures.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let adapter = DojEpsteinAdapter::new(format!("{base_url}/epstein"));
    let page = adapter
        .search("memorandum", SearchOptions::default())
        .await
        .expect("search should parse fixture direct media lead");

    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].source_id, "media-1426281-dl");

    let record = adapter
        .get_record("media-1426281-dl")
        .await
        .expect("media source id should resolve to official media URL");
    assert_eq!(record.source_id, "media-1426281-dl");
    assert_eq!(record.document_url, format!("{base_url}/media/1426281/dl"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let body = include_str!("fixtures/doj_epstein/doj_disclosures.html");
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let adapter = DojEpsteinAdapter::new(format!("{base_url}/epstein"));
    let page = adapter
        .search("antarctica", SearchOptions::default())
        .await
        .expect("search should succeed with empty matches");

    assert!(page.records.is_empty());
    assert!(page
        .warnings
        .iter()
        .any(|warning| warning.contains("no matching records")));
}

#[tokio::test]
async fn get_record_data_set_source_id_prefers_pdf_and_preserves_metadata() {
    let body = include_str!("fixtures/doj_epstein/data_set_1_files.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let adapter = DojEpsteinAdapter::new(format!("{base_url}/epstein"));
    let record = adapter
        .get_record("data-set-1-files")
        .await
        .expect("data set page should parse");

    assert_eq!(record.source_id, "data-set-1-files");
    assert_eq!(record.record_group.as_deref(), Some("efta_data_set"));
    assert_eq!(record.metadata.get("data_set"), Some(&"1".to_owned()));
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[3].role, SourceAssetRole::Image);
    assert_sensitive_warning_mentions_privacy_and_victims(
        record
            .metadata
            .get("source_warning")
            .expect("detail record should carry sensitive source warning"),
    );
    assert!(record
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("official DOJ page/PDF"));
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("Sensitive DOJ Epstein Library"));
    let expected_pdf = format!("{base_url}/epstein/files/DataSet%201/EFTA00000001.pdf");
    assert_eq!(record.pdf_url.as_deref(), Some(expected_pdf.as_str()));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /epstein/doj-disclosures/data-set-1-files HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_official_foia_url() {
    let body = include_str!("fixtures/doj_epstein/foia_cbp.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let record_url =
        format!("{base_url}/epstein/doj-disclosures/foia-customs-and-border-protection-cbp");
    let adapter = DojEpsteinAdapter::new(format!("{base_url}/epstein"));
    let record = adapter
        .get_record(&record_url)
        .await
        .expect("foia detail page should parse");

    assert_eq!(record.record_group.as_deref(), Some("foia"));
    assert_eq!(
        record.metadata.get("component").map(String::as_str),
        Some("Customs and Border Protection (CBP)")
    );
    assert!(record
        .attachments
        .iter()
        .all(|asset| asset.role == SourceAssetRole::Pdf));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with(
        "GET /epstein/doj-disclosures/foia-customs-and-border-protection-cbp HTTP/1.1"
    ));
}

#[tokio::test]
async fn get_record_bop_video_classifies_media_and_keeps_pdf_preferred() {
    let body = include_str!("fixtures/doj_epstein/bop_video_footage.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let adapter = DojEpsteinAdapter::new(format!("{base_url}/epstein"));
    let record = adapter
        .get_record("bop-video-footage")
        .await
        .expect("bop page should parse mixed media assets");

    assert_eq!(record.record_group.as_deref(), Some("prior_doj_disclosure"));
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert!(record
        .attachments
        .iter()
        .any(|asset| asset.role == SourceAssetRole::Other));
    assert!(record
        .attachments
        .iter()
        .any(|asset| asset.mime_type.as_deref() == Some("video/mp4")));
    assert_eq!(
        record
            .attachments
            .iter()
            .filter(|asset| asset
                .mime_type
                .as_deref()
                .is_some_and(|mime| { mime.starts_with("audio/") || mime.starts_with("video/") }))
            .count(),
        2
    );
    assert_eq!(record.metadata.get("media_type"), Some(&"pdf".to_owned()));
    assert_sensitive_warning_mentions_privacy_and_victims(
        record
            .metadata
            .get("source_warning")
            .expect("mixed-media record should carry sensitive source warning"),
    );

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /epstein/doj-disclosures/bop-video-footage HTTP/1.1"));
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/doj_epstein/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let adapter = DojEpsteinAdapter::new(format!("{base_url}/epstein"));
    let err = adapter
        .get_record("data-set-1-files")
        .await
        .expect_err("invalid body should fail as source-changed");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("format may have changed"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn redirect_is_denied_for_disclosures_search() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    )]);

    let adapter = DojEpsteinAdapter::new(format!("{base_url}/epstein"));
    let err = adapter
        .search("data set", SearchOptions::default())
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

fn assert_sensitive_warning_mentions_privacy_and_victims(warning: &str) {
    let lower = warning.to_ascii_lowercase();
    assert!(lower.contains("privacy") || lower.contains("sensitive"));
    assert!(lower.contains("victim-identification"));
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
