use crate::ingest::reconcile::reconcile_derived_artifacts_for_document;
use crate::ingest::reconcile_repair::{plan_derived_artifact_repairs, DerivedArtifactRepairPlan};
use crate::store::{
    AssetInput, AssetRole, BlobKind, ContentAddressedStore, DocumentKey, PageInput, SqliteStore,
    TextSource, UpsertDocument,
};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub(super) struct Fixture {
    pub _tempdir: tempfile::TempDir,
    pub store: SqliteStore,
    pub files: ContentAddressedStore,
    pub document_key: DocumentKey,
    pub canonical_blob_path: PathBuf,
}

pub(super) fn seed_fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let tempdir = tempfile::tempdir()?;
    let data_dir = tempdir.path().join("data");
    let db_dir = data_dir.join("db");
    fs::create_dir_all(&db_dir)?;

    let mut store = SqliteStore::open(db_dir.join("foia.sqlite"))?;
    let files = ContentAddressedStore::new(&data_dir);
    let key = DocumentKey::new("cia_CREST-repair-apply")?;

    store.upsert_document(&UpsertDocument {
        public_id: "cia:CREST-repair-apply".to_owned(),
        document_key: key.clone(),
        source: "cia".to_owned(),
        source_id: "CREST-repair-apply".to_owned(),
        title: "Repair Apply Fixture".to_owned(),
        date: Some("1961-01-01".to_owned()),
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("repair apply fixture".to_owned()),
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

pub(super) fn write_expected_artifacts(
    fixture: &Fixture,
) -> Result<(), Box<dyn std::error::Error>> {
    write_with_parent(
        &fixture
            .files
            .derived_document_text_path(&fixture.document_key),
        "[page 1]\nalpha page one\n\n[page 2]\nbravo page two",
    )?;
    write_with_parent(
        &fixture
            .files
            .derived_page_text_path(&fixture.document_key, 1),
        "alpha page one",
    )?;
    write_with_parent(
        &fixture
            .files
            .derived_page_text_path(&fixture.document_key, 2),
        "bravo page two",
    )?;
    write_with_parent(&ocr_page_path(fixture, 2), "bravo page two")?;
    Ok(())
}

pub(super) fn repair_plan(
    fixture: &Fixture,
) -> Result<DerivedArtifactRepairPlan, Box<dyn std::error::Error>> {
    let report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )?;
    Ok(plan_derived_artifact_repairs(&report))
}

pub(super) fn write_with_parent(
    path: &Path,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(super) fn ocr_page_path(fixture: &Fixture, page_number: u32) -> PathBuf {
    fixture
        .files
        .root()
        .join("ocr")
        .join("pages")
        .join(fixture.document_key.as_str())
        .join(format!("{page_number}.txt"))
}

pub(super) fn row_count(store: &SqliteStore, table: &str) -> i64 {
    store
        .connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count rows")
}

pub(super) fn assert_no_temp_files(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = match path.file_name().and_then(|value| value.to_str()) {
                Some(value) => value,
                None => continue,
            };
            assert!(
                !name.starts_with(".repair-apply-"),
                "unexpected leftover temp file: {}",
                path.display()
            );
        }
    }

    Ok(())
}
