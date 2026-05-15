use foia_search::{
    index::{FtsSearch, SearchQuery},
    ingest::{ingest_text_file, ChunkOptions, IngestDocument},
    store::{DocumentKey, SqliteStore, StoreError, TextSource},
};
use std::path::Path;

fn fixture_path(name: &str) -> String {
    format!(
        "{}/tests/fixtures/ingest/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn fixture_document() -> IngestDocument {
    IngestDocument {
        public_id: "fixture:page-boundaries".to_owned(),
        document_key: DocumentKey::new("fixture_page_boundaries").expect("safe fixture key"),
        source: "fixture".to_owned(),
        source_id: "page-boundaries".to_owned(),
        title: "Page Boundary Fixture".to_owned(),
        date: Some("1960-01-01".to_owned()),
        collection: Some("Integration fixtures".to_owned()),
        record_group: None,
        description: Some("Form-feed-delimited extracted text fixture".to_owned()),
        origin_url: None,
        document_url: None,
        pdf_url: None,
        metadata_json: r#"{"fixture":true}"#.to_owned(),
        citation_note: Some("Fixture citation note".to_owned()),
        terms_note: None,
        text_source: TextSource::EmbeddedPdfText,
    }
}

#[test]
fn text_ingestion_preserves_pages_and_chunks() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    let outcome = ingest_text_file(
        &mut store,
        Path::new(&fixture_path("page_boundaries.txt")),
        fixture_document(),
        &ChunkOptions { target_tokens: 16 },
    )
    .expect("ingest text fixture");

    assert_eq!(outcome.page_count, 3);
    assert_eq!(outcome.chunk_count, 3);

    let document = store
        .get_document_metadata("fixture:page-boundaries")
        .expect("load metadata");
    assert_eq!(document.page_count, 3);

    let pages = store
        .get_page_text("fixture_page_boundaries", 1, 3)
        .expect("load pages");
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0].page_number, 1);
    assert!(pages[0].text.contains("Alpha page one"));
    assert_eq!(pages[1].page_number, 2);
    assert!(pages[1].text.contains("airlift logistics"));
    assert_eq!(pages[2].page_number, 3);
    assert!(pages[2].text.contains("declassification notes"));
}

#[test]
fn text_ingestion_populates_fts_with_page_ranges() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    ingest_text_file(
        &mut store,
        Path::new(&fixture_path("page_boundaries.txt")),
        fixture_document(),
        &ChunkOptions { target_tokens: 16 },
    )
    .expect("ingest text fixture");

    let hits = FtsSearch::new(&store)
        .search(&SearchQuery {
            query: "airlift".to_owned(),
            source: Some("fixture".to_owned()),
            limit: 10,
        })
        .expect("search fixture fts");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].document_key.as_str(), "fixture_page_boundaries");
    assert_eq!(hits[0].page_start, 2);
    assert_eq!(hits[0].page_end, 2);
    assert_eq!(hits[0].chunk_id, "chunk-0002");
}

#[test]
fn text_ingestion_preserves_blank_pages_between_text_pages() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let path = tempdir.path().join("blank-middle.txt");
    std::fs::write(
        &path,
        "First physical page\n\x0C   \n\x0CThird physical page",
    )
    .expect("write blank-page fixture");

    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    let outcome = ingest_text_file(
        &mut store,
        &path,
        fixture_document(),
        &ChunkOptions { target_tokens: 16 },
    )
    .expect("ingest text fixture");

    assert_eq!(outcome.page_count, 3);
    let pages = store
        .get_page_text("fixture_page_boundaries", 1, 3)
        .expect("load pages including blank page");
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[1].page_number, 2);
    assert!(pages[1].text.is_empty());
}

#[test]
fn repeated_text_ingestion_replaces_existing_fts_rows() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    for _ in 0..2 {
        ingest_text_file(
            &mut store,
            Path::new(&fixture_path("page_boundaries.txt")),
            fixture_document(),
            &ChunkOptions { target_tokens: 16 },
        )
        .expect("ingest text fixture");
    }

    let fts_count: i64 = store
        .connection()
        .query_row(
            "SELECT count(*) FROM chunk_fts WHERE document_key = 'fixture_page_boundaries'",
            [],
            |row| row.get(0),
        )
        .expect("count fts rows");
    assert_eq!(fts_count, 3);
}

#[test]
fn empty_text_input_fails_without_partial_document_row() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let empty_path = tempdir.path().join("empty.txt");
    std::fs::write(&empty_path, " \n\x0C\n ").expect("write empty fixture");

    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    let error = ingest_text_file(
        &mut store,
        &empty_path,
        fixture_document(),
        &ChunkOptions { target_tokens: 16 },
    )
    .expect_err("empty extracted text should fail");
    assert!(error.to_string().contains("no pages"));

    let lookup = store
        .get_document_metadata("fixture:page-boundaries")
        .expect_err("failed ingest should not write metadata");
    assert!(matches!(lookup, StoreError::MissingDocument(_)));
}
