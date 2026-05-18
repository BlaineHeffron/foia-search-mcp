use super::ingestion::{enqueue_ingestion_job, parse_document_locator};
use crate::store::{SqliteStore, StoreError};

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
    let mut store = SqliteStore::open_memory().expect("open test store");
    let document_id = "/tmp/doc.pdf";
    let error = enqueue_ingestion_job(&mut store, "refresh", "refresh", document_id, true)
        .expect_err("direct refresh should be rejected before enqueue");

    assert_direct_ingestion_error(document_id, &error.message);
    assert_missing_job(&store, "refresh:/tmp/doc.pdf");
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

fn assert_missing_job(store: &SqliteStore, job_key: &str) {
    let error = store
        .get_ingestion_job_by_key(job_key)
        .expect_err("rejected direct ingestion must not create a job");
    assert!(matches!(error, StoreError::MissingIngestionJob(key) if key == job_key));
}
