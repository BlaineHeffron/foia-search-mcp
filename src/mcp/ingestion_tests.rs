use super::ingestion::{enqueue_ingestion_job, enqueue_refresh_job, parse_document_locator};
use crate::store::{DocumentKey, SqliteStore, StoreError, UpsertDocument};

#[test]
fn source_prefixed_document_id_parses_and_enqueues() {
    let mut store = SqliteStore::open_memory().expect("open test store");

    let locator = parse_document_locator("cia:CREST-123").expect("source ID should parse");
    assert_eq!(locator.source, "cia");
    assert_eq!(locator.source_id, "CREST-123");

    let job = enqueue_ingestion_job(&mut store, "ingest", "ingestion", "cia:CREST-123", false)
        .expect("source-mediated ingestion should enqueue");

    assert_eq!(job.job_key, "ingest:cia:CREST-123");
    assert_eq!(job.source, "cia");
    assert_eq!(job.source_id.as_deref(), Some("CREST-123"));
    assert_eq!(job.target_url, None);
    assert_eq!(job.status, "queued");
}

#[test]
fn source_prefixed_document_id_can_contain_adapter_path_syntax() {
    let locator =
        parse_document_locator("state:FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213")
            .expect("source adapter path-like ID should parse");

    assert_eq!(locator.source, "state");
    assert_eq!(
        locator.source_id,
        "FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213"
    );
}

#[test]
fn direct_url_and_local_file_inputs_are_actionably_rejected() {
    for document_id in [
        "https://example.test/doc.pdf",
        "http://example.test/doc.pdf",
        "file:///tmp/doc.pdf",
        "file:/tmp/doc.pdf",
        "/tmp/doc.pdf",
        "../doc.pdf",
        r"..\doc.pdf",
        "C:\\tmp\\doc.pdf",
        "C:/tmp/doc.pdf",
        r"C:tmp\doc.pdf",
        r"\\server\share\doc.pdf",
    ] {
        let error = parse_document_locator(document_id)
            .expect_err("direct ingestion locator should be rejected");

        assert_direct_ingestion_error(document_id, &error.message);
    }
}

#[test]
fn source_prefixed_direct_or_path_like_inputs_are_rejected() {
    for document_id in [
        "cia:https://example.test/doc.pdf",
        "cia: https://example.test/doc.pdf",
        "cia:file:///tmp/doc.pdf",
        "cia:/tmp/doc.pdf",
        "cia: ../doc.pdf",
        "cia:../doc.pdf",
        r"cia:..\doc.pdf",
        "cia:folder/../doc.pdf",
        "cia:C:\\tmp\\doc.pdf",
        r"cia:C:tmp\doc.pdf",
    ] {
        let error = parse_document_locator(document_id)
            .expect_err("source-prefixed direct ingestion locator should be rejected");

        assert_direct_ingestion_error(document_id, &error.message);
    }
}

#[test]
fn direct_url_and_local_file_rejections_do_not_create_jobs() {
    for document_id in [
        "https://example.test/doc.pdf",
        "http://example.test/doc.pdf",
        "file:///tmp/doc.pdf",
        "/tmp/doc.pdf",
        "../doc.pdf",
        "cia:https://example.test/doc.pdf",
        "cia:../doc.pdf",
        "C:\\tmp\\doc.pdf",
    ] {
        let mut store = SqliteStore::open_memory().expect("open test store");
        let error = enqueue_ingestion_job(&mut store, "ingest", "ingestion", document_id, false)
            .expect_err("direct ingestion should be rejected before enqueue");

        assert_direct_ingestion_error(document_id, &error.message);
        assert_missing_job(&store, &format!("ingest:{document_id}"));
    }
}

#[test]
fn refresh_uses_same_direct_ingestion_rejection_before_enqueue() {
    for document_id in [
        "/tmp/doc.pdf",
        "https://example.test/doc.pdf",
        "cia:https://example.test/doc.pdf",
        "cia:../doc.pdf",
    ] {
        let mut store = SqliteStore::open_memory().expect("open test store");
        let error = enqueue_refresh_job(&mut store, document_id, true)
            .expect_err("direct refresh should be rejected before enqueue");

        assert_direct_ingestion_error(document_id, &error.message);
        assert_missing_job(&store, &format!("refresh:{document_id}"));
    }
}

#[test]
fn refresh_rejects_unknown_or_non_local_document_before_enqueue() {
    let mut store = SqliteStore::open_memory().expect("open test store");

    for document_id in ["cia:CREST-missing", "doc_cia_missing"] {
        let error = enqueue_refresh_job(&mut store, document_id, false)
            .expect_err("refresh should require a local document");

        assert_refresh_missing_error(document_id, &error.message);
    }

    assert_missing_job(&store, "refresh:cia:CREST-missing");
    assert_missing_job(&store, "refresh:doc_cia_missing");
}

#[test]
fn refresh_rejects_unknown_source_with_actionable_local_document_error() {
    let mut store = SqliteStore::open_memory().expect("open test store");

    let error = enqueue_refresh_job(&mut store, "state-dept:123", false)
        .expect_err("refresh should require a known local document, not create source jobs");

    assert_refresh_missing_error("state-dept:123", &error.message);
    assert_missing_job(&store, "refresh:state-dept:123");
}

#[test]
fn refresh_enqueues_durable_job_for_local_source_prefixed_document() {
    let mut store = SqliteStore::open_memory().expect("open test store");
    seed_refresh_document(&store);

    let job = enqueue_refresh_job(&mut store, "cia:CREST-refresh", true)
        .expect("local source-prefixed document should enqueue refresh");

    assert_eq!(job.job_key, "refresh:cia:CREST-refresh");
    assert_eq!(job.source, "cia");
    assert_eq!(job.source_id.as_deref(), Some("CREST-refresh"));
    assert_eq!(job.target_url, None);
    assert_eq!(job.status, "queued");
    assert_eq!(job.stage, "queued");
    assert_eq!(job.progress, 0.0);
    assert!(job
        .next_action
        .as_deref()
        .expect("queued job should include next action")
        .contains("force=true"));

    assert_refresh_outbox_payload(&store, "refresh:cia:CREST-refresh", "refresh", "cia");
}

#[test]
fn refresh_accepts_local_document_key_and_preserves_revalidation_metadata() {
    let mut store = SqliteStore::open_memory().expect("open test store");
    let key = seed_refresh_document(&store);

    let job = enqueue_refresh_job(&mut store, key.as_str(), false)
        .expect("local document_key should enqueue refresh");
    let document = store
        .get_document_metadata("cia:CREST-refresh")
        .expect("seeded document metadata");

    assert_eq!(job.job_key, "refresh:cia:CREST-refresh");
    assert_eq!(job.source, "cia");
    assert_eq!(job.source_id.as_deref(), Some("CREST-refresh"));
    assert_eq!(
        document.citation_note.as_deref(),
        Some("Cite page numbers from refreshed local text.")
    );
    assert_eq!(
        document.terms_note.as_deref(),
        Some("Respect refreshed source terms.")
    );
    assert_refresh_outbox_payload(&store, "refresh:cia:CREST-refresh", "refresh", "cia");
}

fn assert_direct_ingestion_error(document_id: &str, message: &str) {
    assert!(
        message.contains("direct URL and local-file ingestion are disabled by default"),
        "{document_id} error should explain default-deny direct ingestion: {message}"
    );
    assert!(
        message.contains("source-prefixed document_id"),
        "{document_id} error should guide caller to source-mediated IDs: {message}"
    );
    assert!(
        message.contains("search_source or get_source_record"),
        "{document_id} error should name recovery tools: {message}"
    );
}

fn assert_refresh_missing_error(document_id: &str, message: &str) {
    assert!(
        message.contains(
            "refresh_document requires an already-ingested local document_id or document_key"
        ),
        "{document_id} error should explain refresh local-document requirement: {message}"
    );
    assert!(
        message.contains("search_source/get_source_record followed by ingest_document"),
        "{document_id} error should provide recovery path: {message}"
    );
}

fn assert_missing_job(store: &SqliteStore, job_key: &str) {
    let error = store
        .get_ingestion_job_by_key(job_key)
        .expect_err("rejected direct ingestion must not create a job");
    assert!(matches!(error, StoreError::MissingIngestionJob(key) if key == job_key));
}

fn seed_refresh_document(store: &SqliteStore) -> DocumentKey {
    let key = DocumentKey::new("doc_cia_refresh").expect("safe fixture key");
    store
        .upsert_document(&UpsertDocument {
            public_id: "cia:CREST-refresh".to_owned(),
            document_key: key.clone(),
            source: "cia".to_owned(),
            source_id: "CREST-refresh".to_owned(),
            title: "Refresh Fixture".to_owned(),
            date: Some("1962-01-01".to_owned()),
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("Refresh local document fixture".to_owned()),
            origin_url: Some("https://www.cia.gov/readingroom/document/CREST-refresh".to_owned()),
            document_url: Some("https://www.cia.gov/readingroom/document/CREST-refresh".to_owned()),
            pdf_url: Some("https://www.cia.gov/readingroom/docs/CREST-refresh.pdf".to_owned()),
            metadata_json: r#"{"source_metadata":{"release":"fixture"}}"#.to_owned(),
            citation_note: Some("Cite page numbers from refreshed local text.".to_owned()),
            terms_note: Some("Respect refreshed source terms.".to_owned()),
        })
        .expect("seed refresh document");
    key
}

fn assert_refresh_outbox_payload(
    store: &SqliteStore,
    expected_job_key: &str,
    expected_operation: &str,
    expected_source: &str,
) {
    let payload_json: String = store
        .connection()
        .query_row(
            "SELECT payload_json FROM outbox WHERE topic = 'ingestion.job.queued'",
            [],
            |row| row.get(0),
        )
        .expect("queued refresh outbox row");
    let payload: serde_json::Value =
        serde_json::from_str(&payload_json).expect("outbox payload should be JSON");

    assert_eq!(payload["job_key"], expected_job_key);
    assert_eq!(payload["operation"], expected_operation);
    assert_eq!(payload["source"], expected_source);
    assert_eq!(payload["source_id"], "CREST-refresh");
    assert!(payload["target_url"].is_null());
}
