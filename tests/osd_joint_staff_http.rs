use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::sources::{
    osd_joint_staff::OsdJointStaffAdapter, SearchOptions, SourceAdapter, SourceAssetRole,
    SourceError,
};

type ResponseSpec = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[tokio::test]
async fn search_returns_official_osd_joint_staff_reading_room_leads() {
    let listing = include_str!("fixtures/osd_joint_staff/search_results.html");
    let joint_staff = include_str!("fixtures/osd_joint_staff/joint_staff_results.html");
    let (base_url, requests) =
        serve_sequence(vec![response_html(listing), response_html(joint_staff)]);

    let adapter = OsdJointStaffAdapter::new(base_url.clone());
    let page = adapter
        .search("National Military Command", SearchOptions::default())
        .await
        .expect("search should parse OSD/Joint Staff fixtures");

    assert_eq!(page.source, "osd_joint_staff_foia_reading_room");
    assert_eq!(page.records.len(), 1);
    let record = &page.records[0];
    assert_eq!(
        record.source_id,
        "Portals/54/Documents/FOID/Reading%20Room/Joint_Staff/19-F-0260_National_Military_Command_Center_04-19-69.pdf"
    );
    assert_eq!(
        record.collection.as_deref(),
        Some("OSD/Joint Staff FOIA Reading Room")
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
        .contains("official www.esd.whs.mil page"));
    assert!(record
        .metadata
        .get("source_warning")
        .map(String::as_str)
        .unwrap_or_default()
        .contains("Page-level citations"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 2);
    assert!(requests[0]
        .starts_with("GET /Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/ HTTP/1.1"));
    assert!(requests[1].starts_with(
        "GET /Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff/ HTTP/1.1"
    ));
}

#[tokio::test]
async fn search_no_match_returns_warning() {
    let listing = include_str!("fixtures/osd_joint_staff/empty_results.html");
    let joint_staff = include_str!("fixtures/osd_joint_staff/joint_staff_results.html");
    let (base_url, _requests) =
        serve_sequence(vec![response_html(listing), response_html(joint_staff)]);

    let adapter = OsdJointStaffAdapter::new(base_url);
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
    let body = include_str!("fixtures/osd_joint_staff/detail.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);

    let adapter = OsdJointStaffAdapter::new(base_url.clone());
    let record = adapter
        .get_record(
            "osd_joint_staff:Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff",
        )
        .await
        .expect("source id should resolve to official detail page");

    assert_eq!(
        record.source_id,
        "Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff"
    );
    assert_eq!(record.title, "Joint Staff");
    assert_eq!(record.attachments.len(), 3);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[1].role, SourceAssetRole::Pdf);
    assert_eq!(record.attachments[2].role, SourceAssetRole::Html);
    assert_eq!(
        record.metadata.get("pdf_asset_count").map(String::as_str),
        Some("2")
    );
    let expected_pdf = format!(
        "{base_url}/Portals/54/Documents/FOID/Reading%20Room/Joint_Staff/18-F-1152_JP_5-0_Joint_Planning_2020.pdf"
    );
    assert_eq!(record.pdf_url.as_deref(), Some(expected_pdf.as_str()));
    assert!(record
        .terms_note
        .as_deref()
        .unwrap_or_default()
        .contains("Avoid mirrors"));

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with(
        "GET /Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff HTTP/1.1"
    ));
}

#[tokio::test]
async fn get_record_accepts_official_url() {
    let body = include_str!("fixtures/osd_joint_staff/detail.html");
    let (base_url, requests) = serve_sequence(vec![response_html(body)]);
    let record_url =
        format!("{base_url}/Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff/");

    let adapter = OsdJointStaffAdapter::new(base_url);
    let record = adapter
        .get_record(&record_url)
        .await
        .expect("official OSD/Joint Staff URL should resolve");

    assert_eq!(
        record.source_id,
        "Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff"
    );
    assert_eq!(record.document_url, record_url);

    let requests = requests.join().expect("request capture should finish");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with(
        "GET /Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff/ HTTP/1.1"
    ));
}

#[tokio::test]
async fn get_record_direct_official_pdf_returns_single_pdf_asset_without_http() {
    let adapter = OsdJointStaffAdapter::default();

    let record = adapter
        .get_record("https://www.esd.whs.mil/Portals/54/Documents/FOID/Reading%20Room/Joint_Staff/19-F-0260_National_Military_Command_Center_04-19-69.pdf")
        .await
        .expect("direct official OSD/Joint Staff PDF should map without HTTP");

    assert_eq!(record.attachments.len(), 1);
    assert_eq!(record.attachments[0].role, SourceAssetRole::Pdf);
    assert_eq!(
        record.pdf_url.as_deref(),
        Some("https://www.esd.whs.mil/Portals/54/Documents/FOID/Reading%20Room/Joint_Staff/19-F-0260_National_Military_Command_Center_04-19-69.pdf")
    );
}

#[tokio::test]
async fn list_assets_prefers_pdfs_before_non_pdf_assets() {
    let body = include_str!("fixtures/osd_joint_staff/detail.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = OsdJointStaffAdapter::new(base_url);
    let record = adapter
        .get_record("Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff")
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
    let adapter = OsdJointStaffAdapter::default();

    let err = adapter
        .get_record("https://example.com/not-osd-joint-staff")
        .await
        .expect_err("non-official URL should fail");

    match err {
        SourceError::InvalidInput { message, .. } => {
            assert!(message.contains("only accepts official same-origin www.esd.whs.mil FOIA URLs"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_html_returns_source_changed() {
    let body = include_str!("fixtures/osd_joint_staff/invalid_html_response.html");
    let (base_url, _requests) = serve_sequence(vec![response_html(body)]);

    let adapter = OsdJointStaffAdapter::new(base_url);
    let err = adapter
        .get_record("Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff")
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

    let adapter = OsdJointStaffAdapter::new(base_url);
    let err = adapter
        .search("Joint Staff", SearchOptions::default())
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
