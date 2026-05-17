use crate::ingest::reconcile::{
    reconcile_derived_artifacts_for_document, DerivedArtifactIssue, DerivedArtifactIssueKind,
    DerivedArtifactKind, DerivedArtifactReport,
};
use crate::ingest::reconcile_repair::{
    plan_derived_artifact_repairs, DerivedArtifactRepairAction, DerivedArtifactRewriteReason,
};
use crate::store::{
    AssetInput, AssetRole, BlobKind, ContentAddressedStore, DocumentKey, PageInput, SqliteStore,
    TextSource, UpsertDocument,
};
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

#[test]
fn planner_maps_missing_and_stale_issues_to_rewrite_actions() {
    let key = DocumentKey::new("cia_CREST-repair").expect("valid document key");
    let report = DerivedArtifactReport {
        document_key: key,
        issues: vec![
            DerivedArtifactIssue {
                kind: DerivedArtifactKind::PageText,
                issue: DerivedArtifactIssueKind::Missing,
                path: PathBuf::from("/tmp/text/pages/cia_CREST-repair/1.txt"),
                page_number: Some(1),
                detail: "expected derived artifact is absent".to_owned(),
            },
            DerivedArtifactIssue {
                kind: DerivedArtifactKind::DocumentText,
                issue: DerivedArtifactIssueKind::Stale,
                path: PathBuf::from("/tmp/text/documents/cia_CREST-repair.txt"),
                page_number: None,
                detail: "derived artifact content differs from SQLite page state".to_owned(),
            },
        ],
    };

    let plan = plan_derived_artifact_repairs(&report);
    assert_eq!(plan.actions.len(), 2);
    assert!(plan
        .actions
        .contains(&DerivedArtifactRepairAction::RewriteFromSqlite {
            kind: DerivedArtifactKind::PageText,
            path: PathBuf::from("/tmp/text/pages/cia_CREST-repair/1.txt"),
            page_number: Some(1),
            reason: DerivedArtifactRewriteReason::Missing,
        }));
    assert!(plan
        .actions
        .contains(&DerivedArtifactRepairAction::RewriteFromSqlite {
            kind: DerivedArtifactKind::DocumentText,
            path: PathBuf::from("/tmp/text/documents/cia_CREST-repair.txt"),
            page_number: None,
            reason: DerivedArtifactRewriteReason::Stale,
        }));
}

#[test]
fn planner_maps_orphaned_issues_to_manual_review_actions() {
    let key = DocumentKey::new("cia_CREST-repair").expect("valid document key");
    let report = DerivedArtifactReport {
        document_key: key,
        issues: vec![DerivedArtifactIssue {
            kind: DerivedArtifactKind::OcrPageText,
            issue: DerivedArtifactIssueKind::Orphaned,
            path: PathBuf::from("/tmp/ocr/pages/cia_CREST-repair/99.txt"),
            page_number: Some(99),
            detail: "no matching SQLite page state for derived artifact".to_owned(),
        }],
    };

    let plan = plan_derived_artifact_repairs(&report);
    assert_eq!(
        plan.actions,
        vec![DerivedArtifactRepairAction::ManualReview {
            kind: DerivedArtifactKind::OcrPageText,
            path: PathBuf::from("/tmp/ocr/pages/cia_CREST-repair/99.txt"),
            page_number: Some(99),
            detail: "no matching SQLite page state for derived artifact".to_owned(),
        }]
    );
}

#[test]
fn planner_returns_empty_actions_for_clean_report() {
    let plan = plan_derived_artifact_repairs(&DerivedArtifactReport {
        document_key: DocumentKey::new("cia_CREST-clean").expect("valid key"),
        issues: Vec::new(),
    });

    assert!(plan.actions.is_empty());
}

#[test]
fn planner_is_idempotent_and_orders_actions_stably() {
    let key = DocumentKey::new("cia_CREST-repair").expect("valid key");
    let report = DerivedArtifactReport {
        document_key: key.clone(),
        issues: vec![
            DerivedArtifactIssue {
                kind: DerivedArtifactKind::OcrPageText,
                issue: DerivedArtifactIssueKind::Orphaned,
                path: PathBuf::from("/tmp/ocr/pages/cia_CREST-repair/2.txt"),
                page_number: Some(2),
                detail: "orphaned".to_owned(),
            },
            DerivedArtifactIssue {
                kind: DerivedArtifactKind::DocumentText,
                issue: DerivedArtifactIssueKind::Missing,
                path: PathBuf::from("/tmp/text/documents/cia_CREST-repair.txt"),
                page_number: None,
                detail: "missing".to_owned(),
            },
            DerivedArtifactIssue {
                kind: DerivedArtifactKind::PageText,
                issue: DerivedArtifactIssueKind::Stale,
                path: PathBuf::from("/tmp/text/pages/cia_CREST-repair/1.txt"),
                page_number: Some(1),
                detail: "stale".to_owned(),
            },
        ],
    };

    let first = plan_derived_artifact_repairs(&report);
    let second = plan_derived_artifact_repairs(&report);
    assert_eq!(first, second);

    let mut expected_sorted = first.actions.clone();
    expected_sorted.sort();
    assert_eq!(first.actions, expected_sorted);
    assert_eq!(first.document_key, key);
}

#[test]
fn planner_does_not_mutate_files_or_db_rows() {
    let fixture = seed_fixture().expect("seed fixture");
    let stale_document_text_path = fixture
        .files
        .derived_document_text_path(&fixture.document_key);
    if let Some(parent) = stale_document_text_path.parent() {
        fs::create_dir_all(parent).expect("create document parent");
    }
    fs::write(&stale_document_text_path, "stale document text").expect("write stale text");

    let orphaned_ocr_path = fixture
        .files
        .root()
        .join("ocr")
        .join("pages")
        .join(fixture.document_key.as_str())
        .join("99.txt");
    if let Some(parent) = orphaned_ocr_path.parent() {
        fs::create_dir_all(parent).expect("create ocr parent");
    }
    fs::write(&orphaned_ocr_path, "orphaned ocr text").expect("write orphaned ocr");

    let before_documents = row_count(&fixture.store, "documents");
    let before_assets = row_count(&fixture.store, "assets");
    let before_pages = row_count(&fixture.store, "pages");
    let before_chunks = row_count(&fixture.store, "chunks");
    let before_chunk_fts = row_count(&fixture.store, "chunk_fts");
    let before_stale = fs::read_to_string(&stale_document_text_path).expect("read stale before");
    let before_orphan = fs::read_to_string(&orphaned_ocr_path).expect("read orphan before");

    let report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("reconcile report");
    let _plan = plan_derived_artifact_repairs(&report);

    assert_eq!(before_documents, row_count(&fixture.store, "documents"));
    assert_eq!(before_assets, row_count(&fixture.store, "assets"));
    assert_eq!(before_pages, row_count(&fixture.store, "pages"));
    assert_eq!(before_chunks, row_count(&fixture.store, "chunks"));
    assert_eq!(before_chunk_fts, row_count(&fixture.store, "chunk_fts"));
    assert_eq!(
        before_stale,
        fs::read_to_string(&stale_document_text_path).expect("read stale after")
    );
    assert_eq!(
        before_orphan,
        fs::read_to_string(&orphaned_ocr_path).expect("read orphan after")
    );
    assert!(fixture.canonical_blob_path.exists());
}

struct Fixture {
    _tempdir: tempfile::TempDir,
    store: SqliteStore,
    files: ContentAddressedStore,
    document_key: DocumentKey,
    canonical_blob_path: std::path::PathBuf,
}

fn seed_fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let tempdir = tempfile::tempdir()?;
    let data_dir = tempdir.path().join("data");
    let db_dir = data_dir.join("db");
    fs::create_dir_all(&db_dir)?;

    let mut store = SqliteStore::open(db_dir.join("foia.sqlite"))?;
    let files = ContentAddressedStore::new(&data_dir);
    let key = DocumentKey::new("cia_CREST-repair")?;

    store.upsert_document(&UpsertDocument {
        public_id: "cia:CREST-repair".to_owned(),
        document_key: key.clone(),
        source: "cia".to_owned(),
        source_id: "CREST-repair".to_owned(),
        title: "Repair Fixture".to_owned(),
        date: Some("1961-01-01".to_owned()),
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("repair planner fixture".to_owned()),
        origin_url: Some("https://example.test/origin".to_owned()),
        document_url: Some("https://example.test/document".to_owned()),
        pdf_url: Some("https://example.test/document.pdf".to_owned()),
        metadata_json: "{}".to_owned(),
        citation_note: Some("fixture citation".to_owned()),
        terms_note: Some("fixture terms".to_owned()),
    })?;

    store.replace_pages_and_chunks(
        &key,
        &[
            PageInput {
                document_key: key.clone(),
                page_number: 1,
                text: "alpha page one".to_owned(),
                text_source: TextSource::EmbeddedPdfText,
                quality_score: None,
                warnings_json: "[]".to_owned(),
            },
            PageInput {
                document_key: key.clone(),
                page_number: 2,
                text: "bravo page two".to_owned(),
                text_source: TextSource::LocalOcr,
                quality_score: None,
                warnings_json: "[]".to_owned(),
            },
        ],
        &[],
    )?;

    let blob = files.put_reader(BlobKind::Pdf, Cursor::new(b"%PDF fixture".to_vec()))?;
    store.add_asset(&AssetInput {
        document_key: key.clone(),
        asset_url: "https://example.test/document.pdf".to_owned(),
        mime_type: Some("application/pdf".to_owned()),
        role: AssetRole::Pdf,
        sha256: Some(blob.sha256),
        size_bytes: Some(i64::try_from(blob.size_bytes)?),
        etag: Some("fixture-etag".to_owned()),
        last_modified: None,
        fetched_at: None,
        cache_policy: Some("respect_source_headers".to_owned()),
    })?;

    Ok(Fixture {
        _tempdir: tempdir,
        store,
        files,
        document_key: key,
        canonical_blob_path: blob.path,
    })
}

fn row_count(store: &SqliteStore, table: &str) -> i64 {
    store
        .connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count rows")
}
