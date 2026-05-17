use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    pursue::PursueAdapter, SearchOptions, SourceAdapter, SourceAssetRole, SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_uses_ufo_index_and_csv_and_returns_tranche_records() {
    let index_body = include_str!("fixtures/pursue/ufo_index.html");
    let csv_body = include_str!("fixtures/pursue/release001.csv");
    let (base_url, requests) = serve_sequence(vec![
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/html; charset=utf-8")],
            index_body,
        ),
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/csv; charset=utf-8")],
            csv_body,
        ),
    ]);

    let adapter = PursueAdapter::new(base_url.clone());
    let page = adapter
        .search(
            "mission western",
            SearchOptions {
                max_results: 10,
                cursor: None,
            },
        )
        .await
        .expect("search should parse fixtures");

    assert_eq!(page.source, "war_gov_ufo");
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].id, "pursue:release-01:dow-uap-d20");
    assert_eq!(page.records[0].source_id, "release-01:dow-uap-d20");
    assert_eq!(page.records[0].collection.as_deref(), Some("PURSUE"));
    assert_eq!(
        page.records[0]
            .metadata
            .get("release_tranche")
            .map(String::as_str),
        Some("release-01")
    );
    let expected_pdf = format!("{base_url}/medialink/ufo/release_1/dow-uap-d20-mission-report.pdf");
    assert_eq!(
        page.records[0].pdf_url.as_deref(),
        Some(expected_pdf.as_str())
    );
    assert_eq!(page.records[0].attachments.len(), 1);
    assert_eq!(page.records[0].attachments[0].role, SourceAssetRole::Pdf);
    assert!(page.records[0]
        .citation_note
        .as_deref()
        .unwrap_or_default()
        .contains("Verify tranche details"));
    assert!(page.records[0]
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("mixed media"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /ufo/ HTTP/1.1"));
    assert!(
        requests[1].starts_with("GET /Portals/1/Interactive/2026/UFO/uap-release001.csv HTTP/1.1")
    );
}

#[tokio::test]
async fn search_returns_warning_when_no_records_match_query() {
    let index_body = include_str!("fixtures/pursue/ufo_index.html");
    let csv_body = include_str!("fixtures/pursue/release001.csv");
    let (base_url, _requests) = serve_sequence(vec![
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/html; charset=utf-8")],
            index_body,
        ),
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/csv; charset=utf-8")],
            csv_body,
        ),
    ]);

    let adapter = PursueAdapter::new(base_url);
    let page = adapter
        .search("antarctica", SearchOptions::default())
        .await
        .expect("search should return warning page");

    assert!(page.records.is_empty());
    assert_eq!(page.warnings.len(), 1);
    assert!(page.warnings[0].contains("no matching tranche records"));
}

#[tokio::test]
async fn get_record_release_collects_pdf_image_and_video_assets() {
    let body = include_str!("fixtures/pursue/release_page.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let adapter = PursueAdapter::new(base_url);
    let record = adapter
        .get_record("release-01")
        .await
        .expect("release page should parse");

    assert_eq!(record.id, "pursue:release-01");
    assert_eq!(record.source_id, "release-01");
    assert!(record.title.contains("Department of War Releases"));
    assert_eq!(record.attachments.len(), 3);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::Image);
    assert_eq!(record.attachments[2].role, SourceAssetRole::Other);
    assert_eq!(
        record.attachments[2].mime_type.as_deref(),
        Some("video/mp4")
    );

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /ufo/releases/release-01/ HTTP/1.1"));
}

#[tokio::test]
async fn get_record_asset_url_with_release_hint_keeps_asset_provenance() {
    let body = include_str!("fixtures/pursue/release_page.html");
    let (base_url, requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let asset_url = format!("{base_url}/medialink/ufo/release_1/uap-report-001.pdf");
    let adapter = PursueAdapter::new(base_url);
    let record = adapter
        .get_record(&asset_url)
        .await
        .expect("asset URL should resolve to release record");

    assert!(record
        .attachments
        .iter()
        .any(|asset| asset.asset_url == asset_url));
    assert!(record
        .attachments
        .iter()
        .any(|asset| asset.role == SourceAssetRole::Pdf));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /ufo/releases/release-1/ HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_search_returned_source_id() {
    let index_body = include_str!("fixtures/pursue/ufo_index.html");
    let csv_body = include_str!("fixtures/pursue/release001.csv");
    let release_body = include_str!("fixtures/pursue/release_page.html");
    let (base_url, requests) = serve_sequence(vec![
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/html; charset=utf-8")],
            index_body,
        ),
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/csv; charset=utf-8")],
            csv_body,
        ),
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/html; charset=utf-8")],
            release_body,
        ),
    ]);

    let adapter = PursueAdapter::new(base_url);
    let record = adapter
        .get_record("pursue:release-01:dow-uap-d20")
        .await
        .expect("search-returned id should resolve");

    assert_eq!(record.id, "pursue:release-01");
    assert_eq!(record.source_id, "release-01");
    assert!(record
        .attachments
        .iter()
        .any(|asset| asset.label.contains("dow-uap-d20-mission-report")));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /ufo/ HTTP/1.1"));
    assert!(
        requests[1].starts_with("GET /Portals/1/Interactive/2026/UFO/uap-release001.csv HTTP/1.1")
    );
    assert!(requests[2]
        .starts_with("GET /News/Releases/Release/Article/4480582/department-of-war-releases-unidentified-anomalous-phenomena-files-in-historic-t/ HTTP/1.1"));
}

#[tokio::test]
async fn get_record_accepts_official_release_article_url() {
    let index_body = include_str!("fixtures/pursue/ufo_index.html");
    let csv_body = include_str!("fixtures/pursue/release001.csv");
    let (base_url, requests) = serve_sequence(vec![
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/html; charset=utf-8")],
            index_body,
        ),
        (
            "HTTP/1.1 200 OK",
            vec![("Content-Type", "text/csv; charset=utf-8")],
            csv_body,
        ),
    ]);

    let release_article_url = format!(
        "{base_url}/News/Releases/Release/Article/4480582/department-of-war-releases-unidentified-anomalous-phenomena-files-in-historic-t/"
    );
    let adapter = PursueAdapter::new(base_url);
    let record = adapter
        .get_record(&release_article_url)
        .await
        .expect("official release article URL should resolve");

    assert_eq!(record.id, "pursue:release-01:dow-uap-d20");
    assert!(record.document_url.starts_with("http://127.0.0.1:"));
    assert!(record
        .document_url
        .contains("/News/Releases/Release/Article/4480582/"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /ufo/ HTTP/1.1"));
    assert!(
        requests[1].starts_with("GET /Portals/1/Interactive/2026/UFO/uap-release001.csv HTTP/1.1")
    );
}

#[tokio::test]
async fn redirect_is_denied_for_ufo_index_fetch() {
    let redirect_target = "http://127.0.0.1:1/private";
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 302 Found",
        vec![("Location", redirect_target)],
        "",
    )]);

    let adapter = PursueAdapter::new(base_url);
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

#[tokio::test]
async fn release_page_without_assets_returns_source_changed() {
    let body = include_str!("fixtures/pursue/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![(
        "HTTP/1.1 200 OK",
        vec![("Content-Type", "text/html; charset=utf-8")],
        body,
    )]);

    let adapter = PursueAdapter::new(base_url);
    let err = adapter
        .get_record("release-01")
        .await
        .expect_err("missing asset links should fail as source-changed");

    match err {
        SourceError::SourceChanged { message, .. } => {
            assert!(message.contains("format may have changed"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
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
