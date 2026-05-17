use super::reconcile::{reconcile_sqlite_fts_index, FtsReconciliationIssueKind};
use super::reconcile_repair::{
    apply_sqlite_fts_repair_plan, plan_sqlite_fts_repairs, FtsRepairActionKind,
};
use crate::store::{ChunkInput, DocumentKey, PageInput, SqliteStore, TextSource, UpsertDocument};

#[test]
fn repair_plan_rewrites_missing_rows() {
    let (store, key) = seed_store_with_two_chunks();
    delete_fts_row(&store, key.as_str(), "chunk-1");

    let plan = plan_from_store(&store);
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(
        plan.actions[0].action,
        FtsRepairActionKind::RewriteFromCanonical
    );

    let result = apply_sqlite_fts_repair_plan(&store, &plan).expect("apply repair");
    assert_eq!(result.rewritten_count, 1);
    assert_eq!(result.skipped_count, 0);
    assert_eq!(result.manual_review_count, 0);
    assert_clean(&store);
}

#[test]
fn repair_plan_rewrites_stale_rows_from_canonical() {
    let (store, key) = seed_store_with_two_chunks();
    store
        .connection()
        .execute(
            "
            UPDATE chunk_fts
            SET title = ?1, body = ?2, page_start = ?3
            WHERE document_key = ?4 AND chunk_id = ?5
            ",
            ("Wrong", "wrong body", 7_i64, key.as_str(), "chunk-1"),
        )
        .expect("stale fts update");

    let plan = plan_from_store(&store);
    assert_eq!(
        plan.actions[0].action,
        FtsRepairActionKind::RewriteFromCanonical
    );

    let result = apply_sqlite_fts_repair_plan(&store, &plan).expect("apply repair");
    assert_eq!(result.rewritten_count, 1);
    assert_clean(&store);
    assert_eq!(
        fts_body(&store, key.as_str(), "chunk-1"),
        "alpha chunk text"
    );
}

#[test]
fn repair_rewrites_duplicate_rows_to_single_canonical_row() {
    let (store, key) = seed_store_with_two_chunks();
    insert_fts_row(
        &store,
        key.as_str(),
        "chunk-1",
        "cia",
        "Canonical Title",
        "alpha chunk text",
        1,
        1,
    );

    let report = reconcile_sqlite_fts_index(&store).expect("reconcile report");
    assert_eq!(report.issues[0].issue, FtsReconciliationIssueKind::Stale);

    let plan = plan_sqlite_fts_repairs(&report);
    let result = apply_sqlite_fts_repair_plan(&store, &plan).expect("apply repair");
    assert_eq!(result.rewritten_count, 1);
    assert_eq!(fts_identity_count(&store, key.as_str(), "chunk-1"), 1);
    assert_clean(&store);
}

#[test]
fn repair_keeps_orphans_as_manual_review_without_deleting_them() {
    let (store, _key) = seed_store_with_two_chunks();
    insert_fts_row(
        &store,
        "orphan_doc",
        "chunk-orphan",
        "cia",
        "Orphan",
        "orphan body",
        2,
        3,
    );

    let plan = plan_from_store(&store);
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(
        plan.actions[0].action,
        FtsRepairActionKind::ManualReviewOrphan
    );

    let result = apply_sqlite_fts_repair_plan(&store, &plan).expect("apply repair");
    assert_eq!(result.rewritten_count, 0);
    assert_eq!(result.manual_review_count, 1);
    assert_eq!(fts_identity_count(&store, "orphan_doc", "chunk-orphan"), 1);
}

#[test]
fn repair_applies_mixed_drift_and_leaves_manual_orphan() {
    let (store, key) = seed_store_with_two_chunks();
    delete_fts_row(&store, key.as_str(), "chunk-1");
    store
        .connection()
        .execute(
            "UPDATE chunk_fts SET body = ?1 WHERE document_key = ?2 AND chunk_id = ?3",
            ("stale beta", key.as_str(), "chunk-2"),
        )
        .expect("stale second chunk");
    insert_fts_row(
        &store,
        "orphan_doc",
        "chunk-orphan",
        "cia",
        "O",
        "orphan",
        1,
        1,
    );

    let plan = plan_from_store(&store);
    assert_eq!(plan.actions.len(), 3);

    let result = apply_sqlite_fts_repair_plan(&store, &plan).expect("apply repair");
    assert_eq!(result.rewritten_count, 2);
    assert_eq!(result.manual_review_count, 1);

    let report = reconcile_sqlite_fts_index(&store).expect("post repair report");
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].issue, FtsReconciliationIssueKind::Orphaned);
}

#[test]
fn repair_apply_is_idempotent_for_second_apply_of_same_plan() {
    let (store, key) = seed_store_with_two_chunks();
    delete_fts_row(&store, key.as_str(), "chunk-1");
    let plan = plan_from_store(&store);

    let first = apply_sqlite_fts_repair_plan(&store, &plan).expect("first apply");
    let second = apply_sqlite_fts_repair_plan(&store, &plan).expect("second apply");

    assert_eq!(first.rewritten_count, 1);
    assert_eq!(second.rewritten_count, 0);
    assert_eq!(second.skipped_count, 1);
    assert_clean(&store);
}

#[test]
fn repair_apply_does_not_mutate_canonical_tables() {
    let (store, key) = seed_store_with_two_chunks();
    store
        .connection()
        .execute(
            "UPDATE chunk_fts SET body = ?1 WHERE document_key = ?2 AND chunk_id = ?3",
            ("stale alpha", key.as_str(), "chunk-1"),
        )
        .expect("stale fts row");
    let before_documents = row_count(&store, "documents");
    let before_pages = row_count(&store, "pages");
    let before_chunks = row_count(&store, "chunks");
    let before_page_text = page_text(&store, key.as_str(), 1);
    let before_chunk_text = chunk_text(&store, key.as_str(), "chunk-1");

    let plan = plan_from_store(&store);
    apply_sqlite_fts_repair_plan(&store, &plan).expect("apply repair");

    assert_eq!(before_documents, row_count(&store, "documents"));
    assert_eq!(before_pages, row_count(&store, "pages"));
    assert_eq!(before_chunks, row_count(&store, "chunks"));
    assert_eq!(before_page_text, page_text(&store, key.as_str(), 1));
    assert_eq!(
        before_chunk_text,
        chunk_text(&store, key.as_str(), "chunk-1")
    );
    assert_clean(&store);
}

fn seed_store_with_two_chunks() -> (SqliteStore, DocumentKey) {
    let mut store = SqliteStore::open_memory().expect("open in-memory sqlite");
    let key = DocumentKey::new("doc_fts_repair_001").expect("safe key");
    store
        .upsert_document(&UpsertDocument {
            public_id: "cia:CREST-REPAIR-001".to_owned(),
            document_key: key.clone(),
            source: "cia".to_owned(),
            source_id: "CREST-REPAIR-001".to_owned(),
            title: "Canonical Title".to_owned(),
            date: Some("1962-08-01".to_owned()),
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("FTS repair fixture".to_owned()),
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
            &[
                ChunkInput {
                    document_key: key.clone(),
                    chunk_id: "chunk-1".to_owned(),
                    page_start: 1,
                    page_end: 1,
                    text: "alpha chunk text".to_owned(),
                    token_estimate: Some(3),
                    metadata_json: "{}".to_owned(),
                },
                ChunkInput {
                    document_key: key.clone(),
                    chunk_id: "chunk-2".to_owned(),
                    page_start: 1,
                    page_end: 1,
                    text: "beta chunk text".to_owned(),
                    token_estimate: Some(3),
                    metadata_json: "{}".to_owned(),
                },
            ],
        )
        .expect("seed canonical rows");

    (store, key)
}

fn plan_from_store(store: &SqliteStore) -> super::reconcile_repair::FtsRepairPlan {
    let report = reconcile_sqlite_fts_index(store).expect("reconcile report");
    plan_sqlite_fts_repairs(&report)
}

fn assert_clean(store: &SqliteStore) {
    let report = reconcile_sqlite_fts_index(store).expect("reconcile report");
    assert!(
        report.issues.is_empty(),
        "unexpected issues: {:?}",
        report.issues
    );
}

fn delete_fts_row(store: &SqliteStore, document_key: &str, chunk_id: &str) {
    store
        .connection()
        .execute(
            "DELETE FROM chunk_fts WHERE document_key = ?1 AND chunk_id = ?2",
            (document_key, chunk_id),
        )
        .expect("delete fts row");
}

#[allow(clippy::too_many_arguments)]
fn insert_fts_row(
    store: &SqliteStore,
    document_key: &str,
    chunk_id: &str,
    source: &str,
    title: &str,
    body: &str,
    page_start: i64,
    page_end: i64,
) {
    store
        .connection()
        .execute(
            "
            INSERT INTO chunk_fts (document_key, chunk_id, source, title, body, page_start, page_end)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            (document_key, chunk_id, source, title, body, page_start, page_end),
        )
        .expect("insert fts row");
}

fn fts_identity_count(store: &SqliteStore, document_key: &str, chunk_id: &str) -> i64 {
    store
        .connection()
        .query_row(
            "SELECT count(*) FROM chunk_fts WHERE document_key = ?1 AND chunk_id = ?2",
            (document_key, chunk_id),
            |row| row.get(0),
        )
        .expect("fts identity count")
}

fn fts_body(store: &SqliteStore, document_key: &str, chunk_id: &str) -> String {
    store
        .connection()
        .query_row(
            "SELECT body FROM chunk_fts WHERE document_key = ?1 AND chunk_id = ?2",
            (document_key, chunk_id),
            |row| row.get(0),
        )
        .expect("fts body")
}

fn page_text(store: &SqliteStore, document_key: &str, page_number: i64) -> String {
    store
        .connection()
        .query_row(
            "
            SELECT p.text
            FROM pages p
            INNER JOIN documents d ON d.id = p.document_id
            WHERE d.document_key = ?1 AND p.page_number = ?2
            ",
            (document_key, page_number),
            |row| row.get(0),
        )
        .expect("page text")
}

fn chunk_text(store: &SqliteStore, document_key: &str, chunk_id: &str) -> String {
    store
        .connection()
        .query_row(
            "
            SELECT c.text
            FROM chunks c
            INNER JOIN documents d ON d.id = c.document_id
            WHERE d.document_key = ?1 AND c.chunk_id = ?2
            ",
            (document_key, chunk_id),
            |row| row.get(0),
        )
        .expect("chunk text")
}

fn row_count(store: &SqliteStore, table: &str) -> i64 {
    let query = format!("SELECT count(*) FROM {table}");
    store
        .connection()
        .query_row(query.as_str(), [], |row| row.get(0))
        .expect("row count")
}
