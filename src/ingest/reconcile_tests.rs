use crate::ingest::reconcile::{
    reconcile_derived_artifacts_for_document, DerivedArtifactIssueKind, DerivedArtifactKind,
    ReconcileError,
};
use crate::store::{
    AssetInput, AssetRole, BlobKind, ContentAddressedStore, DocumentKey, PageInput, SqliteStore,
    TextSource, UpsertDocument,
};
use std::fs;
use std::io::Cursor;

#[test]
fn reconcile_reports_no_issues_when_artifacts_match_and_is_idempotent() {
    let fixture = seed_fixture().expect("seed fixture");
    write_expected_artifacts(&fixture).expect("write expected artifacts");

    let first = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("first reconciliation");
    let second = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("second reconciliation");

    assert!(first.issues.is_empty(), "{:?}", first.issues);
    assert_eq!(first.issues, second.issues);
}

#[test]
fn reconcile_detects_missing_expected_artifacts() {
    let fixture = seed_fixture().expect("seed fixture");

    let report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("reconciliation");

    assert!(has_issue(
        &report,
        DerivedArtifactKind::DocumentText,
        DerivedArtifactIssueKind::Missing,
        None,
    ));
    assert!(has_issue(
        &report,
        DerivedArtifactKind::PageText,
        DerivedArtifactIssueKind::Missing,
        Some(1),
    ));
    assert!(has_issue(
        &report,
        DerivedArtifactKind::PageText,
        DerivedArtifactIssueKind::Missing,
        Some(2),
    ));
    assert!(has_issue(
        &report,
        DerivedArtifactKind::OcrPageText,
        DerivedArtifactIssueKind::Missing,
        Some(2),
    ));
}

#[test]
fn reconcile_detects_stale_expected_artifacts() {
    let fixture = seed_fixture().expect("seed fixture");
    write_expected_artifacts(&fixture).expect("write expected artifacts");

    fs::write(
        fixture
            .files
            .derived_document_text_path(&fixture.document_key),
        "stale document text",
    )
    .expect("write stale document text");
    fs::write(
        fixture
            .files
            .derived_page_text_path(&fixture.document_key, 1),
        "stale page text",
    )
    .expect("write stale page text");
    fs::write(ocr_page_path(&fixture, 2), "stale ocr text").expect("write stale ocr text");

    let report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("reconciliation");

    assert!(has_issue(
        &report,
        DerivedArtifactKind::DocumentText,
        DerivedArtifactIssueKind::Stale,
        None,
    ));
    assert!(has_issue(
        &report,
        DerivedArtifactKind::PageText,
        DerivedArtifactIssueKind::Stale,
        Some(1),
    ));
    assert!(has_issue(
        &report,
        DerivedArtifactKind::OcrPageText,
        DerivedArtifactIssueKind::Stale,
        Some(2),
    ));
}

#[test]
fn reconcile_detects_orphaned_artifacts() {
    let fixture = seed_fixture().expect("seed fixture");
    write_expected_artifacts(&fixture).expect("write expected artifacts");

    let extra_text = fixture
        .files
        .root()
        .join("text")
        .join("pages")
        .join(fixture.document_key.as_str())
        .join("99.txt");
    fs::write(&extra_text, "orphaned page").expect("write orphaned text page");

    let extra_ocr = ocr_page_path(&fixture, 1);
    fs::write(&extra_ocr, "orphaned ocr").expect("write orphaned ocr page");

    let invalid_name = fixture
        .files
        .root()
        .join("ocr")
        .join("pages")
        .join(fixture.document_key.as_str())
        .join("not-a-page.txt");
    fs::write(&invalid_name, "orphaned ocr").expect("write invalid orphaned ocr page");

    let report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("reconciliation");

    assert!(has_issue(
        &report,
        DerivedArtifactKind::PageText,
        DerivedArtifactIssueKind::Orphaned,
        Some(99),
    ));
    assert!(has_issue(
        &report,
        DerivedArtifactKind::OcrPageText,
        DerivedArtifactIssueKind::Orphaned,
        Some(1),
    ));
    assert!(has_issue(
        &report,
        DerivedArtifactKind::OcrPageText,
        DerivedArtifactIssueKind::Orphaned,
        None,
    ));
}

#[test]
fn reconcile_detects_orphaned_non_file_page_entries() {
    let fixture = seed_fixture().expect("seed fixture");
    write_expected_artifacts(&fixture).expect("write expected artifacts");

    let text_pages_dir = fixture
        .files
        .root()
        .join("text")
        .join("pages")
        .join(fixture.document_key.as_str());
    let ocr_pages_dir = fixture
        .files
        .root()
        .join("ocr")
        .join("pages")
        .join(fixture.document_key.as_str());
    fs::create_dir_all(text_pages_dir.join("99.txt")).expect("create unexpected text directory");
    fs::create_dir_all(ocr_pages_dir.join("scratch")).expect("create unexpected ocr directory");

    let report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("reconciliation");

    assert!(has_issue(
        &report,
        DerivedArtifactKind::PageText,
        DerivedArtifactIssueKind::Orphaned,
        Some(99),
    ));
    assert!(has_issue(
        &report,
        DerivedArtifactKind::OcrPageText,
        DerivedArtifactIssueKind::Orphaned,
        None,
    ));
}

#[test]
fn reconcile_handles_absent_artifact_paths() {
    let fixture = seed_fixture().expect("seed fixture");

    let report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("reconciliation with absent paths");

    assert!(!report.issues.is_empty());
    assert!(report
        .issues
        .iter()
        .all(|issue| issue.issue == DerivedArtifactIssueKind::Missing));
}

#[test]
fn reconcile_reports_directory_shape_errors() {
    let fixture = seed_fixture().expect("seed fixture");

    let bad_dir_path = fixture
        .files
        .root()
        .join("text")
        .join("pages")
        .join(fixture.document_key.as_str());
    fs::create_dir_all(
        bad_dir_path
            .parent()
            .expect("document page path should have parent"),
    )
    .expect("create parent");
    fs::write(&bad_dir_path, "not a directory").expect("create invalid dir sentinel");

    let error = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect_err("invalid directory shape should fail");

    match error {
        ReconcileError::Io {
            operation, path, ..
        } => {
            assert_eq!(operation, "read_dir");
            assert_eq!(path, bad_dir_path);
        }
        other => panic!("unexpected error variant: {other}"),
    }
}

#[test]
fn reconcile_does_not_delete_canonical_rows_or_blobs() {
    let fixture = seed_fixture().expect("seed fixture");
    let before_documents = row_count(&fixture.store, "documents");
    let before_assets = row_count(&fixture.store, "assets");
    let before_pages = row_count(&fixture.store, "pages");
    let before_chunks = row_count(&fixture.store, "chunks");
    let before_chunk_fts = row_count(&fixture.store, "chunk_fts");

    let report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("reconciliation");

    assert!(!report.issues.is_empty());
    assert_eq!(before_documents, row_count(&fixture.store, "documents"));
    assert_eq!(before_assets, row_count(&fixture.store, "assets"));
    assert_eq!(before_pages, row_count(&fixture.store, "pages"));
    assert_eq!(before_chunks, row_count(&fixture.store, "chunks"));
    assert_eq!(before_chunk_fts, row_count(&fixture.store, "chunk_fts"));
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
    let key = DocumentKey::new("cia_CREST-reconcile")?;

    store.upsert_document(&UpsertDocument {
        public_id: "cia:CREST-reconcile".to_owned(),
        document_key: key.clone(),
        source: "cia".to_owned(),
        source_id: "CREST-reconcile".to_owned(),
        title: "Reconcile Fixture".to_owned(),
        date: Some("1963-01-01".to_owned()),
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("derived artifact reconciliation fixture".to_owned()),
        origin_url: Some("https://www.cia.gov/readingroom/document/CREST-reconcile".to_owned()),
        document_url: Some("https://www.cia.gov/readingroom/document/CREST-reconcile".to_owned()),
        pdf_url: Some("https://www.cia.gov/readingroom/docs/CREST-reconcile.pdf".to_owned()),
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
                text: "alpha page one text".to_owned(),
                text_source: TextSource::EmbeddedPdfText,
                quality_score: None,
                warnings_json: "[]".to_owned(),
            },
            PageInput {
                document_key: key.clone(),
                page_number: 2,
                text: "bravo page two text".to_owned(),
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
        asset_url: "https://www.cia.gov/readingroom/docs/CREST-reconcile.pdf".to_owned(),
        mime_type: Some("application/pdf".to_owned()),
        role: AssetRole::Pdf,
        sha256: Some(blob.sha256),
        size_bytes: Some(i64::try_from(blob.size_bytes)?),
        etag: Some("etag-fixture".to_owned()),
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

fn write_expected_artifacts(fixture: &Fixture) -> Result<(), Box<dyn std::error::Error>> {
    let document_text = "[page 1]\nalpha page one text\n\n[page 2]\nbravo page two text";
    let document_path = fixture
        .files
        .derived_document_text_path(&fixture.document_key);
    if let Some(parent) = document_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(document_path, document_text)?;

    let page1 = fixture
        .files
        .derived_page_text_path(&fixture.document_key, 1);
    let page2 = fixture
        .files
        .derived_page_text_path(&fixture.document_key, 2);
    if let Some(parent) = page1.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(page1, "alpha page one text")?;
    fs::write(page2, "bravo page two text")?;

    let ocr2 = ocr_page_path(fixture, 2);
    if let Some(parent) = ocr2.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(ocr2, "bravo page two text")?;

    Ok(())
}

fn ocr_page_path(fixture: &Fixture, page_number: u32) -> std::path::PathBuf {
    fixture
        .files
        .root()
        .join("ocr")
        .join("pages")
        .join(fixture.document_key.as_str())
        .join(format!("{page_number}.txt"))
}

fn has_issue(
    report: &crate::ingest::reconcile::DerivedArtifactReport,
    kind: DerivedArtifactKind,
    issue: DerivedArtifactIssueKind,
    page_number: Option<u32>,
) -> bool {
    report
        .issues
        .iter()
        .any(|entry| entry.kind == kind && entry.issue == issue && entry.page_number == page_number)
}

fn row_count(store: &SqliteStore, table: &str) -> i64 {
    store
        .connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count rows")
}
