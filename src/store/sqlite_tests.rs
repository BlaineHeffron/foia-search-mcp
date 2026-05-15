use super::{
    AssetInput, AssetRole, DocumentKey, NewIngestionJob, PageInput, SqliteStore, StoreError,
    TextSource, UpsertDocument,
};

fn insert_document_with_pages() -> (SqliteStore, DocumentKey) {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    let key = DocumentKey::new("doc_cia_lookup").expect("safe key");
    store
        .upsert_document(&UpsertDocument {
            public_id: "cia:CREST-lookup".to_owned(),
            document_key: key.clone(),
            source: "cia".to_owned(),
            source_id: "CREST-lookup".to_owned(),
            title: "Lookup Test".to_owned(),
            date: Some("1961-01-01".to_owned()),
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("A local lookup fixture".to_owned()),
            origin_url: Some("https://example.test/origin".to_owned()),
            document_url: Some("https://example.test/document".to_owned()),
            pdf_url: Some("https://example.test/document.pdf".to_owned()),
            metadata_json: r#"{"classification":"declassified"}"#.to_owned(),
            citation_note: Some("Cite page numbers from local OCR.".to_owned()),
            terms_note: Some("Public domain source terms.".to_owned()),
        })
        .expect("insert document");
    store
        .replace_pages_and_chunks(
            &key,
            &[
                PageInput {
                    document_key: key.clone(),
                    page_number: 1,
                    text: "Page one text".to_owned(),
                    text_source: TextSource::EmbeddedPdfText,
                    quality_score: Some(0.95),
                    warnings_json: "[]".to_owned(),
                },
                PageInput {
                    document_key: key.clone(),
                    page_number: 2,
                    text: "Page two text".to_owned(),
                    text_source: TextSource::LocalOcr,
                    quality_score: Some(0.80),
                    warnings_json: "[]".to_owned(),
                },
            ],
            &[],
        )
        .expect("replace pages");
    (store, key)
}

#[test]
fn migration_is_idempotent() {
    let store = SqliteStore::open_memory().expect("open in-memory store");
    store.migrate().expect("second migration run");

    let table_count: i64 = store
        .connection()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type IN ('table', 'index')",
            [],
            |row| row.get(0),
        )
        .expect("count sqlite objects");
    assert!(table_count > 0);
}

#[test]
fn document_key_must_not_be_source_id_or_public_id() {
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let document = UpsertDocument {
        public_id: "cia:CREST/unsafe/id".to_owned(),
        document_key: DocumentKey::new("CREST_unsafe_id").expect("safe key"),
        source: "cia".to_owned(),
        source_id: "CREST/unsafe/id".to_owned(),
        title: "Test".to_owned(),
        date: None,
        collection: None,
        record_group: None,
        description: None,
        origin_url: None,
        document_url: None,
        pdf_url: None,
        metadata_json: "{}".to_owned(),
        citation_note: None,
        terms_note: None,
    };

    let stored = store.upsert_document(&document).expect("insert document");
    assert_eq!(stored.document_key.as_str(), "CREST_unsafe_id");
    assert_ne!(stored.document_key.as_str(), stored.source_id);
}

#[test]
fn unsafe_document_keys_are_rejected() {
    assert!(DocumentKey::new("CREST/unsafe/id").is_err());
    assert!(DocumentKey::new("../escape").is_err());
    assert!(DocumentKey::new("").is_err());
}

#[test]
fn asset_upsert_returns_stable_row_id() {
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let key = DocumentKey::new("doc_cia_asset").expect("safe key");
    store
        .upsert_document(&UpsertDocument {
            public_id: "cia:asset-test".to_owned(),
            document_key: key.clone(),
            source: "cia".to_owned(),
            source_id: "asset-test".to_owned(),
            title: "Asset Test".to_owned(),
            date: None,
            collection: None,
            record_group: None,
            description: None,
            origin_url: None,
            document_url: None,
            pdf_url: None,
            metadata_json: "{}".to_owned(),
            citation_note: None,
            terms_note: None,
        })
        .expect("insert document");

    let first = store
        .add_asset(&AssetInput {
            document_key: key.clone(),
            asset_url: "https://www.cia.gov/readingroom/docs/test.pdf".to_owned(),
            mime_type: Some("application/pdf".to_owned()),
            role: AssetRole::Pdf,
            sha256: Some("a".repeat(64)),
            size_bytes: Some(10),
            etag: None,
            last_modified: None,
            fetched_at: None,
            cache_policy: Some("respect_source_headers".to_owned()),
        })
        .expect("insert asset");
    let second = store
        .add_asset(&AssetInput {
            document_key: key,
            asset_url: "https://www.cia.gov/readingroom/docs/test.pdf".to_owned(),
            mime_type: Some("application/pdf".to_owned()),
            role: AssetRole::Pdf,
            sha256: Some("b".repeat(64)),
            size_bytes: Some(20),
            etag: None,
            last_modified: None,
            fetched_at: None,
            cache_policy: Some("respect_source_headers".to_owned()),
        })
        .expect("update asset");

    assert_eq!(first, second);
}

#[test]
fn document_metadata_can_be_loaded_by_public_id_or_key() {
    let (store, key) = insert_document_with_pages();

    let by_public_id = store
        .get_document_metadata("cia:CREST-lookup")
        .expect("lookup by public id");
    let by_key = store
        .get_document_metadata(key.as_str())
        .expect("lookup by document key");

    assert_eq!(by_public_id.document_key, key);
    assert_eq!(by_public_id.public_id, "cia:CREST-lookup");
    assert_eq!(by_public_id.title, "Lookup Test");
    assert_eq!(by_public_id.page_count, 2);
    assert_eq!(
        by_public_id.metadata_json,
        r#"{"classification":"declassified"}"#
    );
    assert_eq!(by_public_id.public_id, by_key.public_id);
    assert_eq!(by_public_id.page_count, by_key.page_count);
}

#[test]
fn page_text_returns_inclusive_ordered_range() {
    let (store, key) = insert_document_with_pages();

    let pages = store
        .get_page_text(key.as_str(), 1, 2)
        .expect("lookup page range");

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].page_number, 1);
    assert_eq!(pages[0].text, "Page one text");
    assert_eq!(pages[0].text_source, "embedded_pdf_text");
    assert_eq!(pages[1].page_number, 2);
    assert_eq!(pages[1].text, "Page two text");
    assert_eq!(pages[1].text_source, "local_ocr");
}

#[test]
fn page_text_reports_missing_document_and_missing_pages() {
    let (store, key) = insert_document_with_pages();

    let missing_document = store
        .get_page_text("cia:missing", 1, 1)
        .expect_err("missing document");
    assert!(matches!(missing_document, StoreError::MissingDocument(_)));

    let missing_pages = store
        .get_page_text(key.as_str(), 2, 3)
        .expect_err("missing page range");
    assert!(matches!(missing_pages, StoreError::MissingPages { .. }));
}

#[test]
fn page_text_validates_range() {
    let (store, key) = insert_document_with_pages();

    let zero_page = store
        .get_page_text(key.as_str(), 0, 1)
        .expect_err("zero page");
    assert!(matches!(zero_page, StoreError::InvalidPageRange(_)));

    let inverted = store
        .get_page_text(key.as_str(), 2, 1)
        .expect_err("inverted page range");
    assert!(matches!(inverted, StoreError::InvalidPageRange(_)));
}

#[test]
fn ingestion_job_creation_is_idempotent_and_outbox_is_stable() {
    let mut store = SqliteStore::open_memory().expect("open in-memory store");
    let job = NewIngestionJob {
        job_key: "ingest:cia:CREST-123".to_owned(),
        operation: "ingest".to_owned(),
        source: "cia".to_owned(),
        source_id: Some("CREST-123".to_owned()),
        target_url: None,
        next_action: "Queued for ingestion pipeline; asset download and extraction are pending."
            .to_owned(),
    };

    let first = store.create_ingestion_job(&job).expect("create job");
    let second = store
        .create_ingestion_job(&job)
        .expect("return existing job");

    assert_eq!(first.job_key, second.job_key);
    assert_eq!(second.status, "queued");
    assert_eq!(second.progress, 0.0);

    let job_count: i64 = store
        .connection()
        .query_row(
            "SELECT count(*) FROM ingestion_jobs WHERE job_key = ?1",
            [&job.job_key],
            |row| row.get(0),
        )
        .expect("count jobs");
    let outbox_count: i64 = store
        .connection()
        .query_row(
            "SELECT count(*) FROM outbox WHERE topic = 'ingestion.job.queued'",
            [],
            |row| row.get(0),
        )
        .expect("count outbox rows");

    assert_eq!(job_count, 1);
    assert_eq!(outbox_count, 1);
}

#[test]
fn missing_ingestion_job_returns_typed_error() {
    let store = SqliteStore::open_memory().expect("open in-memory store");
    let err = store
        .get_ingestion_job_by_key("ingest:missing")
        .expect_err("missing job should error");

    assert!(matches!(err, StoreError::MissingIngestionJob(key) if key == "ingest:missing"));
}
