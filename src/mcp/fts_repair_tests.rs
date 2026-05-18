use crate::mcp::fts_repair::{
    apply_sqlite_fts_repairs, plan_sqlite_fts_repairs, report_sqlite_fts_drift,
};
use crate::store::{ChunkInput, DocumentKey, PageInput, SqliteStore, TextSource, UpsertDocument};

#[test]
fn report_and_plan_surface_remain_dry_run() -> Result<(), Box<dyn std::error::Error>> {
    let (store, key) = seed_store()?;
    delete_fts_row(&store, key.as_str(), "chunk-1")?;
    update_fts_body(&store, key.as_str(), "chunk-2", "stale beta")?;
    insert_orphan_fts_row(&store)?;

    let report = report_sqlite_fts_drift(&store).expect("report sqlite fts drift");
    assert_eq!(report.canonical_chunk_count, 2);
    assert_eq!(report.issue_count, 3);
    assert!(report.issues.iter().any(|issue| issue.issue == "missing"));
    assert!(report.issues.iter().any(|issue| issue.issue == "stale"));
    assert!(report.issues.iter().any(|issue| issue.issue == "orphaned"));
    assert!(report
        .next_actions
        .iter()
        .any(|next| next.contains("Plan the drift")));
    assert_eq!(fts_body(&store, key.as_str(), "chunk-2")?, "stale beta");
    assert_eq!(fts_identity_count(&store, "orphan_doc", "chunk-orphan")?, 1);

    let plan = plan_sqlite_fts_repairs(&store).expect("plan sqlite fts repairs");
    assert_eq!(plan.action_count, 3);
    assert_eq!(plan.rewrite_count, 2);
    assert_eq!(plan.manual_review_count, 1);
    assert!(plan
        .actions
        .iter()
        .any(|action| action.action == "rewrite_from_canonical"));
    assert!(plan
        .actions
        .iter()
        .any(|action| action.action == "manual_review"));
    assert!(plan
        .next_actions
        .iter()
        .any(|next| next.contains("confirm: apply sqlite fts repairs")));
    assert_eq!(fts_body(&store, key.as_str(), "chunk-2")?, "stale beta");
    assert_eq!(fts_identity_count(&store, "orphan_doc", "chunk-orphan")?, 1);

    Ok(())
}

#[test]
fn apply_rejects_bad_confirmation_without_mutation() -> Result<(), Box<dyn std::error::Error>> {
    let (store, key) = seed_store()?;
    update_fts_body(&store, key.as_str(), "chunk-1", "stale alpha")?;

    let missing_confirmation_error =
        apply_sqlite_fts_repairs(&store, "").expect_err("missing confirmation should fail");
    assert!(missing_confirmation_error
        .to_string()
        .contains("confirmation must exactly match 'apply sqlite fts repairs'"));
    assert_eq!(fts_body(&store, key.as_str(), "chunk-1")?, "stale alpha");

    let error = apply_sqlite_fts_repairs(&store, "apply fts repairs")
        .expect_err("wrong confirmation should fail");
    assert!(error
        .to_string()
        .contains("confirmation must exactly match 'apply sqlite fts repairs'"));
    assert_eq!(fts_body(&store, key.as_str(), "chunk-1")?, "stale alpha");

    Ok(())
}

#[test]
fn apply_rewrites_safe_rows_and_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let (store, key) = seed_store()?;
    delete_fts_row(&store, key.as_str(), "chunk-1")?;
    update_fts_body(&store, key.as_str(), "chunk-2", "stale beta")?;

    let first = apply_sqlite_fts_repairs(&store, "apply sqlite fts repairs")
        .expect("apply sqlite fts repairs");
    assert_eq!(first.issue_count, 2);
    assert_eq!(first.rewritten, 2);
    assert_eq!(first.already_current, 0);
    assert_eq!(first.skipped_manual_review, 0);
    assert_eq!(
        fts_body(&store, key.as_str(), "chunk-1")?,
        "alpha chunk text"
    );
    assert_eq!(
        fts_body(&store, key.as_str(), "chunk-2")?,
        "beta chunk text"
    );

    let second = apply_sqlite_fts_repairs(&store, "apply sqlite fts repairs")
        .expect("second apply sqlite fts repairs");
    assert_eq!(second.issue_count, 0);
    assert_eq!(second.rewritten, 0);
    assert_eq!(second.already_current, 0);
    assert_eq!(second.skipped_manual_review, 0);
    assert!(second
        .next_actions
        .iter()
        .any(|next| next.contains("No SQLite FTS index drift")));

    Ok(())
}

#[test]
fn orphan_rows_remain_manual_review_and_are_not_deleted() -> Result<(), Box<dyn std::error::Error>>
{
    let (store, _key) = seed_store()?;
    insert_orphan_fts_row(&store)?;

    let apply = apply_sqlite_fts_repairs(&store, "apply sqlite fts repairs")
        .expect("apply sqlite fts repairs");
    assert_eq!(apply.rewritten, 0);
    assert_eq!(apply.skipped_manual_review, 1);
    assert_eq!(fts_identity_count(&store, "orphan_doc", "chunk-orphan")?, 1);
    assert!(apply
        .next_actions
        .iter()
        .any(|next| next.contains("Manual-review chunk_fts rows remain")));

    Ok(())
}

#[test]
fn mcp_apply_does_not_mutate_canonical_tables() -> Result<(), Box<dyn std::error::Error>> {
    let (store, key) = seed_store()?;
    update_fts_body(&store, key.as_str(), "chunk-1", "stale alpha")?;
    let before_documents = row_count(&store, "documents")?;
    let before_pages = row_count(&store, "pages")?;
    let before_chunks = row_count(&store, "chunks")?;
    let before_page_text = page_text(&store, key.as_str(), 1)?;
    let before_chunk_text = chunk_text(&store, key.as_str(), "chunk-1")?;

    apply_sqlite_fts_repairs(&store, "apply sqlite fts repairs").expect("apply sqlite fts repairs");

    assert_eq!(before_documents, row_count(&store, "documents")?);
    assert_eq!(before_pages, row_count(&store, "pages")?);
    assert_eq!(before_chunks, row_count(&store, "chunks")?);
    assert_eq!(before_page_text, page_text(&store, key.as_str(), 1)?);
    assert_eq!(
        before_chunk_text,
        chunk_text(&store, key.as_str(), "chunk-1")?
    );

    Ok(())
}

fn seed_store() -> Result<(SqliteStore, DocumentKey), Box<dyn std::error::Error>> {
    let mut store = SqliteStore::open_memory()?;
    let key = DocumentKey::new("doc_mcp_fts_repair_001")?;
    store.upsert_document(&UpsertDocument {
        public_id: "cia:CREST-MCP-FTS-001".to_owned(),
        document_key: key.clone(),
        source: "cia".to_owned(),
        source_id: "CREST-MCP-FTS-001".to_owned(),
        title: "Canonical FTS Title".to_owned(),
        date: Some("1962-08-01".to_owned()),
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("MCP FTS repair fixture".to_owned()),
        origin_url: None,
        document_url: None,
        pdf_url: None,
        metadata_json: "{}".to_owned(),
        citation_note: None,
        terms_note: None,
    })?;
    store.replace_pages_and_chunks(
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
    )?;

    Ok((store, key))
}

fn delete_fts_row(
    store: &SqliteStore,
    document_key: &str,
    chunk_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    store.connection().execute(
        "DELETE FROM chunk_fts WHERE document_key = ?1 AND chunk_id = ?2",
        (document_key, chunk_id),
    )?;
    Ok(())
}

fn update_fts_body(
    store: &SqliteStore,
    document_key: &str,
    chunk_id: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    store.connection().execute(
        "UPDATE chunk_fts SET body = ?1 WHERE document_key = ?2 AND chunk_id = ?3",
        (body, document_key, chunk_id),
    )?;
    Ok(())
}

fn insert_orphan_fts_row(store: &SqliteStore) -> Result<(), Box<dyn std::error::Error>> {
    store.connection().execute(
        "
        INSERT INTO chunk_fts (document_key, chunk_id, source, title, body, page_start, page_end)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        (
            "orphan_doc",
            "chunk-orphan",
            "cia",
            "Orphan",
            "orphan body",
            2_i64,
            3_i64,
        ),
    )?;
    Ok(())
}

fn fts_identity_count(
    store: &SqliteStore,
    document_key: &str,
    chunk_id: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    let count = store.connection().query_row(
        "SELECT count(*) FROM chunk_fts WHERE document_key = ?1 AND chunk_id = ?2",
        (document_key, chunk_id),
        |row| row.get(0),
    )?;
    Ok(count)
}

fn fts_body(
    store: &SqliteStore,
    document_key: &str,
    chunk_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let body = store.connection().query_row(
        "SELECT body FROM chunk_fts WHERE document_key = ?1 AND chunk_id = ?2",
        (document_key, chunk_id),
        |row| row.get(0),
    )?;
    Ok(body)
}

fn page_text(
    store: &SqliteStore,
    document_key: &str,
    page_number: i64,
) -> Result<String, Box<dyn std::error::Error>> {
    let text = store.connection().query_row(
        "
        SELECT p.text
        FROM pages p
        INNER JOIN documents d ON d.id = p.document_id
        WHERE d.document_key = ?1 AND p.page_number = ?2
        ",
        (document_key, page_number),
        |row| row.get(0),
    )?;
    Ok(text)
}

fn chunk_text(
    store: &SqliteStore,
    document_key: &str,
    chunk_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let text = store.connection().query_row(
        "
        SELECT c.text
        FROM chunks c
        INNER JOIN documents d ON d.id = c.document_id
        WHERE d.document_key = ?1 AND c.chunk_id = ?2
        ",
        (document_key, chunk_id),
        |row| row.get(0),
    )?;
    Ok(text)
}

fn row_count(store: &SqliteStore, table: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let query = format!("SELECT count(*) FROM {table}");
    let count = store
        .connection()
        .query_row(query.as_str(), [], |row| row.get(0))?;
    Ok(count)
}
