use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use foia_search::{
    ingest::download::{cache_key, AssetDownloadRequest, AssetDownloader, DownloadError},
    sources::{SourceAsset, SourceAssetRole},
    store::{CachePolicy, CacheStore, ContentAddressedStore, SqliteStore},
};

#[tokio::test]
async fn respect_source_headers_persists_blob_cache_and_revalidates() {
    let body = b"%PDF fixture";
    let (asset_url, requests) = serve_sequence(vec![
        FixtureResponse::ok(body)
            .with_header("Content-Type", "application/pdf")
            .with_header("ETag", r#""asset-v1""#)
            .with_header("Last-Modified", "Tue, 04 Mar 2025 12:00:00 GMT"),
        FixtureResponse::status("HTTP/1.1 304 Not Modified")
            .with_header("ETag", r#""asset-v1""#)
            .with_header("Last-Modified", "Tue, 04 Mar 2025 12:00:00 GMT"),
    ]);
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let cache = CacheStore::new(&store);
    let tempdir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(tempdir.path());
    let downloader = AssetDownloader::new().expect("downloader");
    let asset = pdf_asset(&asset_url);

    let first = downloader
        .download(
            &files,
            &cache,
            request("cia", &asset, CachePolicy::RespectSourceHeaders, false),
        )
        .await
        .expect("first download");
    let second = downloader
        .download(
            &files,
            &cache,
            request("cia", &asset, CachePolicy::RespectSourceHeaders, false),
        )
        .await
        .expect("revalidated download");

    assert_eq!(first.status_code, 200);
    assert_eq!(first.etag.as_deref(), Some(r#""asset-v1""#));
    assert_eq!(
        first.last_modified.as_deref(),
        Some("Tue, 04 Mar 2025 12:00:00 GMT")
    );
    assert!(first.path.exists());
    assert_eq!(second.status_code, 304);
    assert_eq!(second.sha256, first.sha256);
    assert_eq!(second.path, first.path);
    assert_eq!(second.size_bytes, body.len() as u64);

    let entry = cache
        .get(&cache_key("cia", &asset_url))
        .expect("read cache")
        .expect("cache entry");
    assert_eq!(entry.body_sha256.as_deref(), Some(first.sha256.as_str()));
    assert_eq!(
        entry.body_path.as_deref(),
        Some(first.path.to_str().expect("utf8 path"))
    );
    assert_eq!(entry.etag.as_deref(), Some(r#""asset-v1""#));

    let provenance: serde_json::Value =
        serde_json::from_str(&first.provenance_json).expect("provenance json");
    assert_eq!(provenance["cache_status"], "fetched");
    assert_eq!(
        provenance["body_path"].as_str(),
        Some(first.path.to_string_lossy().as_ref())
    );
    assert!(first.response_headers_json.contains("etag"));

    let requests = requests.join().expect("server requests");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /asset.pdf "));
    assert!(requests[1].contains("\r\nif-none-match: \"asset-v1\"\r\n"));
    assert!(requests[1].contains("\r\nif-modified-since: Tue, 04 Mar 2025 12:00:00 GMT\r\n"));
}

#[tokio::test]
async fn do_not_persist_downloads_blob_but_leaves_cache_empty() {
    let (asset_url, requests) = serve_sequence(vec![
        FixtureResponse::ok(b"source policy body").with_header("ETag", r#""private-v1""#)
    ]);
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let cache = CacheStore::new(&store);
    let tempdir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(tempdir.path());
    let downloader = AssetDownloader::new().expect("downloader");
    let asset = pdf_asset(&asset_url);

    let downloaded = downloader
        .download(
            &files,
            &cache,
            request("nara_catalog", &asset, CachePolicy::DoNotPersist, false),
        )
        .await
        .expect("download with do-not-persist policy");

    assert!(downloaded.path.exists());
    assert_eq!(downloaded.cache_policy, CachePolicy::DoNotPersist);
    assert!(cache
        .get(&cache_key("nara_catalog", &asset_url))
        .expect("read cache")
        .is_none());
    let cache_rows: i64 = store
        .connection()
        .query_row("SELECT count(*) FROM cache_entries", [], |row| row.get(0))
        .expect("count cache rows");
    assert_eq!(cache_rows, 0);
    assert_eq!(requests.join().expect("server requests").len(), 1);
}

#[tokio::test]
async fn http_error_does_not_write_cache_or_blob() {
    let (asset_url, requests) = serve_sequence(vec![FixtureResponse::status(
        "HTTP/1.1 503 Service Unavailable",
    )
    .with_body(b"unavailable")]);
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let cache = CacheStore::new(&store);
    let tempdir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(tempdir.path());
    let downloader = AssetDownloader::new().expect("downloader");
    let asset = pdf_asset(&asset_url);

    let error = downloader
        .download(
            &files,
            &cache,
            request("cia", &asset, CachePolicy::RespectSourceHeaders, false),
        )
        .await
        .expect_err("503 should fail");

    assert!(matches!(error, DownloadError::HttpStatus { .. }));
    assert!(cache
        .get(&cache_key("cia", &asset_url))
        .expect("read cache")
        .is_none());
    assert!(!tempdir.path().join("blobs").exists());
    assert_eq!(requests.join().expect("server requests").len(), 1);
}

#[tokio::test]
async fn repeated_same_body_downloads_deduplicate_in_file_store() {
    let body = b"same pdf bytes";
    let (asset_url, requests) = serve_sequence(vec![
        FixtureResponse::ok(body).with_header("Content-Type", "application/pdf"),
        FixtureResponse::ok(body).with_header("Content-Type", "application/pdf"),
    ]);
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let cache = CacheStore::new(&store);
    let tempdir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(tempdir.path());
    let downloader = AssetDownloader::new().expect("downloader");
    let asset = pdf_asset(&asset_url);

    let first = downloader
        .download(
            &files,
            &cache,
            request("cia", &asset, CachePolicy::RespectSourceHeaders, true),
        )
        .await
        .expect("first download");
    let second = downloader
        .download(
            &files,
            &cache,
            request("cia", &asset, CachePolicy::RespectSourceHeaders, true),
        )
        .await
        .expect("second download");

    assert_eq!(first.sha256, second.sha256);
    assert_eq!(first.path, second.path);
    assert_eq!(count_files(tempdir.path().join("blobs")), 1);
    assert_eq!(requests.join().expect("server requests").len(), 2);
}

#[tokio::test]
async fn oversized_response_is_rejected_before_persisting() {
    let (asset_url, requests) = serve_sequence(vec![
        FixtureResponse::ok(b"tiny").with_header("Content-Length", "9")
    ]);
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let cache = CacheStore::new(&store);
    let tempdir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(tempdir.path());
    let client = reqwest::Client::builder().build().expect("client");
    let downloader = AssetDownloader::with_client(client, 8);
    let asset = pdf_asset(&asset_url);

    let error = downloader
        .download(
            &files,
            &cache,
            request("cia", &asset, CachePolicy::RespectSourceHeaders, false),
        )
        .await
        .expect_err("content-length over limit should fail");

    assert!(matches!(error, DownloadError::TooLarge { .. }));
    assert!(!tempdir.path().join("blobs").exists());
    assert_eq!(requests.join().expect("server requests").len(), 1);
}

#[tokio::test]
async fn redirect_response_is_not_followed_or_persisted() {
    let (asset_url, requests) = serve_sequence(vec![FixtureResponse::status("HTTP/1.1 302 Found")
        .with_header("Location", "http://127.0.0.1:1/private.pdf")]);
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let cache = CacheStore::new(&store);
    let tempdir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(tempdir.path());
    let downloader = AssetDownloader::new().expect("downloader");
    let asset = pdf_asset(&asset_url);

    let error = downloader
        .download(
            &files,
            &cache,
            request("cia", &asset, CachePolicy::RespectSourceHeaders, false),
        )
        .await
        .expect_err("redirects should not be followed implicitly");

    assert!(matches!(error, DownloadError::HttpStatus { .. }));
    assert!(cache
        .get(&cache_key("cia", &asset_url))
        .expect("read cache")
        .is_none());
    assert!(!tempdir.path().join("blobs").exists());
    assert_eq!(requests.join().expect("server requests").len(), 1);
}

fn request<'a>(
    source: &'a str,
    asset: &'a SourceAsset,
    cache_policy: CachePolicy,
    force: bool,
) -> AssetDownloadRequest<'a> {
    AssetDownloadRequest {
        source,
        asset,
        cache_policy,
        force,
    }
}

fn pdf_asset(asset_url: &str) -> SourceAsset {
    SourceAsset {
        asset_url: asset_url.to_owned(),
        label: "PDF".to_owned(),
        mime_type: Some("application/pdf".to_owned()),
        role: SourceAssetRole::Pdf,
    }
}

#[derive(Clone)]
struct FixtureResponse {
    status_line: &'static str,
    headers: Vec<(&'static str, &'static str)>,
    body: Vec<u8>,
}

impl FixtureResponse {
    fn ok(body: &[u8]) -> Self {
        Self::status("HTTP/1.1 200 OK").with_body(body)
    }

    fn status(status_line: &'static str) -> Self {
        Self {
            status_line,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn with_header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((name, value));
        self
    }

    fn with_body(mut self, body: &[u8]) -> Self {
        self.body = body.to_vec();
        self
    }
}

fn serve_sequence(responses: Vec<FixtureResponse>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _addr) = listener.accept().expect("test server should accept");
            let mut buffer = [0; 4096];
            let read = stream.read(&mut buffer).expect("test server should read");
            requests.push(String::from_utf8_lossy(&buffer[..read]).to_string());

            let has_content_length = response
                .headers
                .iter()
                .any(|(name, _value)| name.eq_ignore_ascii_case("content-length"));
            let mut response_head = format!("{}\r\nConnection: close\r\n", response.status_line);
            if !has_content_length {
                response_head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
            }
            for (name, value) in response.headers {
                response_head.push_str(name);
                response_head.push_str(": ");
                response_head.push_str(value);
                response_head.push_str("\r\n");
            }
            response_head.push_str("\r\n");
            stream
                .write_all(response_head.as_bytes())
                .expect("test server should write headers");
            stream
                .write_all(&response.body)
                .expect("test server should write body");
        }
        requests
    });

    (format!("http://{addr}/asset.pdf"), handle)
}

fn count_files(path: impl AsRef<std::path::Path>) -> usize {
    let path = path.as_ref();
    if path.is_file() {
        return 1;
    }
    if !path.exists() {
        return 0;
    }
    fs::read_dir(path)
        .expect("read dir")
        .map(|entry| count_files(entry.expect("dir entry").path()))
        .sum()
}
