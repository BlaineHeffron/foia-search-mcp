use super::reconcile::{reconcile_sqlite_fts_index, FtsReconciliationIssueKind};
use crate::store::{ChunkInput, DocumentKey, PageInput, SqliteStore, TextSource, UpsertDocument};

#[test]
fn reconcile_sqlite_fts_is_clean_when_canonical_and_fts_match() {
    let (store, _key) = seed_store_with_single_chunk();
    let report = reconcile_sqlite_fts_index(&store).expect("reconcile report");

    assert_eq!(report.canonical_chunk_count, 1);
    assert_eq!(report.chunk_fts_row_count, 1);
    assert!(report.issues.is_empty());
}

#[test]
fn reconcile_sqlite_fts_reports_missing_chunk_fts_rows() {
    let (store, key) = seed_store_with_single_chunk();
    store
        .connection()
        .execute(
            "DELETE FROM chunk_fts WHERE document_key = ?1 AND chunk_id = ?2",
            (key.as_str(), "chunk-1"),
        )
        .expect("delete chunk_fts row");

    let report = reconcile_sqlite_fts_index(&store).expect("reconcile report");
    assert_eq!(report.canonical_chunk_count, 1);
    assert_eq!(report.chunk_fts_row_count, 0);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].issue, FtsReconciliationIssueKind::Missing);
    assert_eq!(report.issues[0].document_key, key.as_str());
    assert_eq!(report.issues[0].chunk_id, "chunk-1");
}

#[test]
fn reconcile_sqlite_fts_reports_orphaned_chunk_fts_rows() {
    let (store, _key) = seed_store_with_single_chunk();
    store
        .connection()
        .execute(
            "
            INSERT INTO chunk_fts (document_key, chunk_id, source, title, body, page_start, page_end)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            (
                "orphan_doc",
                "chunk-orphan",
                "cia",
                "Orphaned Title",
                "orphaned body text",
                2_i64,
                3_i64,
            ),
        )
        .expect("insert orphan chunk_fts row");

    let report = reconcile_sqlite_fts_index(&store).expect("reconcile report");

    assert_eq!(report.canonical_chunk_count, 1);
    assert_eq!(report.chunk_fts_row_count, 2);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].issue, FtsReconciliationIssueKind::Orphaned);
    assert_eq!(report.issues[0].document_key, "orphan_doc");
    assert_eq!(report.issues[0].chunk_id, "chunk-orphan");
}

#[test]
fn reconcile_sqlite_fts_reports_stale_rows_when_fields_drift() {
    let (store, key) = seed_store_with_single_chunk();
    store
        .connection()
        .execute(
            "
            UPDATE chunk_fts
            SET source = ?1, title = ?2, body = ?3, page_start = ?4, page_end = ?5
            WHERE document_key = ?6 AND chunk_id = ?7
            ",
            (
                "nara",
                "Wrong Title",
                "wrong body",
                9_i64,
                10_i64,
                key.as_str(),
                "chunk-1",
            ),
        )
        .expect("mutate chunk_fts row");

    let report = reconcile_sqlite_fts_index(&store).expect("reconcile report");
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].issue, FtsReconciliationIssueKind::Stale);
    assert_eq!(report.issues[0].document_key, key.as_str());
    assert!(report.issues[0].detail.contains("source"));
    assert!(report.issues[0].detail.contains("title"));
    assert!(report.issues[0].detail.contains("body"));
    assert!(report.issues[0].detail.contains("page_start"));
    assert!(report.issues[0].detail.contains("page_end"));
}

#[test]
fn reconcile_sqlite_fts_reports_duplicate_rows_for_matching_chunk() {
    let (store, key) = seed_store_with_single_chunk();
    store
        .connection()
        .execute(
            "
            INSERT INTO chunk_fts (document_key, chunk_id, source, title, body, page_start, page_end)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            (
                key.as_str(),
                "chunk-1",
                "cia",
                "Canonical Title",
                "alpha chunk text",
                1_i64,
                1_i64,
            ),
        )
        .expect("insert duplicate matching chunk_fts row");

    let report = reconcile_sqlite_fts_index(&store).expect("reconcile report");
    assert_eq!(report.canonical_chunk_count, 1);
    assert_eq!(report.chunk_fts_row_count, 2);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].issue, FtsReconciliationIssueKind::Stale);
    assert_eq!(report.issues[0].document_key, key.as_str());
    assert_eq!(report.issues[0].chunk_id, "chunk-1");
    assert!(report.issues[0].detail.contains("expected 1"));
}

#[test]
fn reconcile_sqlite_fts_returns_error_when_derived_table_is_unreadable() {
    let (store, _key) = seed_store_with_single_chunk();
    store
        .connection()
        .execute("DROP TABLE chunk_fts", [])
        .expect("drop derived chunk_fts table");

    let error = reconcile_sqlite_fts_index(&store).expect_err("reconcile error");
    assert!(error.to_string().contains("chunk_fts"));
}

#[test]
fn reconcile_sqlite_fts_report_is_stable_and_idempotent() {
    let (store, key) = seed_store_with_single_chunk();
    store
        .connection()
        .execute(
            "DELETE FROM chunk_fts WHERE document_key = ?1 AND chunk_id = ?2",
            (key.as_str(), "chunk-1"),
        )
        .expect("delete canonical fts row");
    store
        .connection()
        .execute(
            "
            INSERT INTO chunk_fts (document_key, chunk_id, source, title, body, page_start, page_end)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            ("zzz_orphan", "chunk-z", "cia", "Z", "orphan z", 1_i64, 1_i64),
        )
        .expect("insert first orphan row");
    store
        .connection()
        .execute(
            "
            INSERT INTO chunk_fts (document_key, chunk_id, source, title, body, page_start, page_end)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            ("aaa_orphan", "chunk-a", "cia", "A", "orphan a", 1_i64, 1_i64),
        )
        .expect("insert second orphan row");

    let first = reconcile_sqlite_fts_index(&store).expect("first reconciliation");
    let second = reconcile_sqlite_fts_index(&store).expect("second reconciliation");

    assert_eq!(first, second);
    assert_eq!(
        first
            .issues
            .iter()
            .map(|issue| issue.document_key.as_str())
            .collect::<Vec<_>>(),
        vec!["aaa_orphan", key.as_str(), "zzz_orphan"]
    );
}

#[test]
fn reconcile_sqlite_fts_does_not_mutate_store_rows() {
    let (store, key) = seed_store_with_single_chunk();
    store
        .connection()
        .execute(
            "
            UPDATE chunk_fts
            SET body = ?1
            WHERE document_key = ?2 AND chunk_id = ?3
            ",
            ("stale-body", key.as_str(), "chunk-1"),
        )
        .expect("stale update");

    let before_documents = row_count(&store, "documents");
    let before_pages = row_count(&store, "pages");
    let before_chunks = row_count(&store, "chunks");
    let before_chunk_fts = row_count(&store, "chunk_fts");

    let report = reconcile_sqlite_fts_index(&store).expect("reconcile report");
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].issue, FtsReconciliationIssueKind::Stale);

    assert_eq!(before_documents, row_count(&store, "documents"));
    assert_eq!(before_pages, row_count(&store, "pages"));
    assert_eq!(before_chunks, row_count(&store, "chunks"));
    assert_eq!(before_chunk_fts, row_count(&store, "chunk_fts"));
}

fn seed_store_with_single_chunk() -> (SqliteStore, DocumentKey) {
    let mut store = SqliteStore::open_memory().expect("open in-memory sqlite");
    let key = DocumentKey::new("doc_fts_reconcile_001").expect("safe key");
    store
        .upsert_document(&UpsertDocument {
            public_id: "cia:CREST-FTS-001".to_owned(),
            document_key: key.clone(),
            source: "cia".to_owned(),
            source_id: "CREST-FTS-001".to_owned(),
            title: "Canonical Title".to_owned(),
            date: Some("1962-08-01".to_owned()),
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("FTS reconcile fixture".to_owned()),
            origin_url: None,
            document_url: None,
            pdf_url: None,
            metadata_json: "{}".to_owned(),
            citation_note: None,
            terms_note: None,
        })
        .expect("upsert canonical document");
    store
        .replace_pages_and_chunks(
            &key,
            &[PageInput {
                document_key: key.clone(),
                page_number: 1,
                text: "alpha page text".to_owned(),
                text_source: TextSource::EmbeddedPdfText,
                quality_score: Some(0.9),
                warnings_json: "[]".to_owned(),
            }],
            &[ChunkInput {
                document_key: key.clone(),
                chunk_id: "chunk-1".to_owned(),
                page_start: 1,
                page_end: 1,
                text: "alpha chunk text".to_owned(),
                token_estimate: Some(3),
                metadata_json: "{}".to_owned(),
            }],
        )
        .expect("seed canonical pages/chunks/fts rows");

    (store, key)
}

fn row_count(store: &SqliteStore, table: &str) -> i64 {
    let query = format!("SELECT count(*) FROM {table}");
    store
        .connection()
        .query_row(query.as_str(), [], |row| row.get(0))
        .expect("row count")
}
