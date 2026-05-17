use foia_search::store::{
    AssetInput, AssetRole, ChunkInput, DocumentKey, PageInput, SqliteStore, TextSource,
    UpsertDocument,
};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;
use std::time::Duration;

pub fn seed_partial_local_state(
    data_dir: &Path,
    job_key: &str,
    lease_expiry: &str,
    asset_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_dir = data_dir.join("db");
    fs::create_dir_all(&db_dir)?;
    let mut store = SqliteStore::open(db_dir.join("foia.sqlite"))?;
    let key = DocumentKey::new("cia_CREST-restart")?;
    store.upsert_document(&UpsertDocument {
        public_id: "cia:CREST-restart".to_owned(),
        document_key: key.clone(),
        source: "cia".to_owned(),
        source_id: "CREST-restart".to_owned(),
        title: "Restart Fixture".to_owned(),
        date: Some("1962-08-01".to_owned()),
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("stale partial state".to_owned()),
        origin_url: Some("https://www.cia.gov/readingroom/document/CREST-restart".to_owned()),
        document_url: Some("https://www.cia.gov/readingroom/document/CREST-restart".to_owned()),
        pdf_url: Some(asset_url.to_owned()),
        metadata_json: "{}".to_owned(),
        citation_note: Some("stale fixture citation".to_owned()),
        terms_note: Some("stale fixture terms".to_owned()),
    })?;
    store.replace_pages_and_chunks(
        &key,
        &[PageInput {
            document_key: key.clone(),
            page_number: 1,
            text: "stale page text".to_owned(),
            text_source: TextSource::EmbeddedPdfText,
            quality_score: None,
            warnings_json: "[]".to_owned(),
        }],
        &[ChunkInput {
            document_key: key.clone(),
            chunk_id: "chunk-1".to_owned(),
            page_start: 1,
            page_end: 1,
            text: "stale searchable chunk".to_owned(),
            token_estimate: Some(3),
            metadata_json: "{}".to_owned(),
        }],
    )?;
    store.add_asset(&AssetInput {
        document_key: key,
        asset_url: asset_url.to_owned(),
        mime_type: Some("text/plain".to_owned()),
        role: AssetRole::Other,
        sha256: Some("0".repeat(64)),
        size_bytes: Some(1),
        etag: None,
        last_modified: None,
        fetched_at: None,
        cache_policy: Some("respect_source_headers".to_owned()),
    })?;
    store.connection().execute(
        "
        UPDATE ingestion_jobs
        SET status = 'running',
            stage = 'extracting_text',
            progress = 0.60,
            attempts = 1,
            lease_owner = 'seed-worker',
            lease_expires_at = ?2,
            document_id = (SELECT id FROM documents WHERE public_id = 'cia:CREST-restart')
        WHERE job_key = ?1
        ",
        (job_key, lease_expiry),
    )?;
    Ok(())
}

pub fn count_text_rows_with_substring(store: &SqliteStore, table: &str, needle: &str) -> i64 {
    let query = format!("SELECT count(*) FROM {table} WHERE text LIKE '%' || ?1 || '%'");
    store
        .connection()
        .query_row(query.as_str(), [needle], |row| row.get(0))
        .expect("count rows with substring")
}

pub fn chunk_fts_match_count(store: &SqliteStore, term: &str) -> i64 {
    store
        .connection()
        .query_row(
            "SELECT count(*) FROM chunk_fts WHERE chunk_fts MATCH ?1",
            [term],
            |row| row.get(0),
        )
        .expect("count fts matches")
}

pub struct SingleResponseFixtureServer {
    pub asset_url: String,
    _listener_thread: thread::JoinHandle<()>,
}

impl SingleResponseFixtureServer {
    pub fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let addr = listener.local_addr().expect("fixture server address");
        let listener_thread = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                read_http_request(&mut stream);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        Self {
            asset_url: format!("http://{addr}/fixture.txt"),
            _listener_thread: listener_thread,
        }
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut buffer = [0_u8; 512];
    while request.windows(4).all(|window| window != b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                request.extend_from_slice(&buffer[..read]);
                if request.len() > 16 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
