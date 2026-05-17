use crate::mcp::repair::{
    apply_derived_artifact_repairs, plan_derived_artifact_repairs, report_derived_artifact_drift,
};
use crate::store::{
    ChunkInput, ContentAddressedStore, DocumentKey, PageInput, SqliteStore, TextSource,
    UpsertDocument,
};
use std::fs;

#[test]
fn report_and_plan_surface_remain_dry_run() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = seed_fixture()?;

    let report =
        report_derived_artifact_drift(&fixture.store, &fixture.files, fixture.document_id())
            .expect("report derived artifact drift");
    assert_eq!(report.issue_count, 6);
    assert!(report.issues.iter().any(|issue| issue.issue == "orphaned"));
    assert!(report
        .next_actions
        .iter()
        .any(|next| next.contains("Plan the drift")));

    let plan = plan_derived_artifact_repairs(&fixture.store, &fixture.files, fixture.document_id())
        .expect("plan derived artifact repairs");
    assert_eq!(plan.action_count, 6);
    assert_eq!(plan.rewrite_count, 5);
    assert_eq!(plan.manual_review_count, 1);
    assert!(plan
        .actions
        .iter()
        .any(|action| action.action == "manual_review"));
    assert!(plan.next_actions.iter().any(|next| next
        .contains("confirm: apply derived artifact repairs for cia:CREST-repair-surface")));
    assert!(fixture.document_text_path().exists());
    assert!(fixture.page_text_path(1).exists());
    assert!(fixture.ocr_page_text_path(1).exists());
    assert!(fixture.orphan_page_text_path(99).exists());

    Ok(())
}

#[test]
fn compact_next_actions_are_operator_gated() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = seed_fixture()?;
    let confirmation = format!(
        "apply derived artifact repairs for {}",
        fixture.document_id()
    );

    let report =
        report_derived_artifact_drift(&fixture.store, &fixture.files, fixture.document_id())
            .expect("report derived artifact drift");
    assert_eq!(report.next_actions.len(), 1);
    assert!(report.next_actions[0].contains("Plan the drift"));
    assert!(report.next_actions[0].contains("manual-review only"));
    assert!(fixture.document_text_path().exists());

    let plan = plan_derived_artifact_repairs(&fixture.store, &fixture.files, fixture.document_id())
        .expect("plan derived artifact repairs");
    assert_eq!(plan.next_actions.len(), 1);
    assert!(plan.next_actions[0].contains("Review manual-review items"));
    assert!(plan.next_actions[0].contains("apply skips them"));
    assert!(plan.next_actions[0].contains(&confirmation));
    assert!(fixture.document_text_path().exists());

    let wrong_confirmation_error = apply_derived_artifact_repairs(
        &fixture.store,
        &fixture.files,
        fixture.document_id(),
        "apply derived artifact repairs",
    )
    .expect_err("wrong confirmation should fail");
    assert!(wrong_confirmation_error.to_string().contains(&confirmation));
    assert_eq!(
        fs::read_to_string(fixture.document_text_path()).expect("read unrepaired document text"),
        "stale document text"
    );

    Ok(())
}

#[test]
fn apply_requires_confirmation_and_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = seed_fixture()?;

    let expected_confirmation = format!(
        "apply derived artifact repairs for {}",
        fixture.document_id()
    );
    let error = apply_derived_artifact_repairs(
        &fixture.store,
        &fixture.files,
        fixture.document_id(),
        "definitely not confirmed",
    )
    .expect_err("apply should require explicit confirmation");
    assert!(error
        .to_string()
        .contains("confirmation must exactly match"));

    let first = apply_derived_artifact_repairs(
        &fixture.store,
        &fixture.files,
        fixture.document_id(),
        &expected_confirmation,
    )
    .expect("apply derived artifact repairs");
    assert_eq!(first.issue_count, 6);
    assert_eq!(first.rewritten, 5);
    assert_eq!(first.already_current, 0);
    assert_eq!(first.skipped_manual_review, 1);
    assert!(first
        .next_actions
        .iter()
        .any(|next| next.contains("Manual-review items remain")));

    assert_eq!(
        fs::read_to_string(fixture.document_text_path()).expect("read repaired document text"),
        "[page 1]\nAlpha page one\n\n[page 2]\nAlpha page two"
    );
    assert_eq!(
        fs::read_to_string(fixture.page_text_path(1)).expect("read repaired page text"),
        "Alpha page one"
    );
    assert_eq!(
        fs::read_to_string(fixture.ocr_page_text_path(1)).expect("read repaired ocr text"),
        "Alpha page one"
    );
    assert!(fixture.orphan_page_text_path(99).exists());

    let second = apply_derived_artifact_repairs(
        &fixture.store,
        &fixture.files,
        fixture.document_id(),
        &expected_confirmation,
    )
    .expect("second apply derived artifact repairs");
    assert_eq!(second.rewritten, 0);
    assert_eq!(second.already_current, 0);
    assert_eq!(second.skipped_manual_review, 1);

    Ok(())
}

#[test]
fn manual_review_items_are_not_deleted() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = seed_fixture()?;
    let confirmation = format!(
        "apply derived artifact repairs for {}",
        fixture.document_id()
    );

    apply_derived_artifact_repairs(
        &fixture.store,
        &fixture.files,
        fixture.document_id(),
        &confirmation,
    )
    .expect("apply derived artifact repairs");

    assert!(fixture.orphan_page_text_path(99).exists());
    assert!(fixture.document_text_path().exists());
    assert!(fixture.page_text_path(2).exists());

    Ok(())
}

struct RepairFixture {
    _data_dir: tempfile::TempDir,
    store: SqliteStore,
    files: ContentAddressedStore,
    document_key: DocumentKey,
}

impl RepairFixture {
    fn document_id(&self) -> &str {
        "cia:CREST-repair-surface"
    }

    fn document_text_path(&self) -> std::path::PathBuf {
        self.files.derived_document_text_path(&self.document_key)
    }

    fn page_text_path(&self, page_number: u32) -> std::path::PathBuf {
        self.files
            .derived_page_text_path(&self.document_key, page_number)
    }

    fn ocr_page_text_path(&self, page_number: u32) -> std::path::PathBuf {
        self.files
            .root()
            .join("ocr")
            .join("pages")
            .join(self.document_key.as_str())
            .join(format!("{page_number}.txt"))
    }

    fn orphan_page_text_path(&self, page_number: u32) -> std::path::PathBuf {
        self.files
            .root()
            .join("text")
            .join("pages")
            .join(self.document_key.as_str())
            .join(format!("{page_number}.txt"))
    }
}

fn seed_fixture() -> Result<RepairFixture, Box<dyn std::error::Error>> {
    let data_dir = tempfile::tempdir()?;
    fs::create_dir_all(data_dir.path().join("db"))?;
    let mut store = SqliteStore::open(data_dir.path().join("db").join("foia.sqlite"))?;
    let files = ContentAddressedStore::new(data_dir.path());
    let document_key = DocumentKey::new("cia_CREST-repair-surface")?;
    let document_id = "cia:CREST-repair-surface".to_owned();

    store.upsert_document(&UpsertDocument {
        public_id: document_id,
        document_key: document_key.clone(),
        source: "cia".to_owned(),
        source_id: "CREST-repair-surface".to_owned(),
        title: "Repair surface fixture".to_owned(),
        date: Some("1963-01-01".to_owned()),
        collection: Some("CIA Reading Room".to_owned()),
        record_group: None,
        description: Some("repair surface fixture".to_owned()),
        origin_url: Some("https://example.invalid/origin".to_owned()),
        document_url: Some("https://example.invalid/document".to_owned()),
        pdf_url: Some("https://example.invalid/document.pdf".to_owned()),
        metadata_json: "{}".to_owned(),
        citation_note: Some("cite the original".to_owned()),
        terms_note: Some("respect source terms".to_owned()),
    })?;

    store.replace_pages_and_chunks(
        &document_key,
        &[
            PageInput {
                document_key: document_key.clone(),
                page_number: 1,
                text: "Alpha page one".to_owned(),
                text_source: TextSource::LocalOcr,
                quality_score: Some(0.9),
                warnings_json: "[]".to_owned(),
            },
            PageInput {
                document_key: document_key.clone(),
                page_number: 2,
                text: "Alpha page two".to_owned(),
                text_source: TextSource::LocalOcr,
                quality_score: Some(0.8),
                warnings_json: "[]".to_owned(),
            },
        ],
        &[ChunkInput {
            document_key: document_key.clone(),
            chunk_id: "alpha-1".to_owned(),
            page_start: 1,
            page_end: 2,
            text: "Alpha page one Alpha page two".to_owned(),
            token_estimate: Some(6),
            metadata_json: "{}".to_owned(),
        }],
    )?;

    fs::create_dir_all(files.root().join("text").join("documents"))?;
    fs::create_dir_all(
        files
            .root()
            .join("text")
            .join("pages")
            .join(document_key.as_str()),
    )?;
    fs::create_dir_all(
        files
            .root()
            .join("ocr")
            .join("pages")
            .join(document_key.as_str()),
    )?;

    fs::write(
        files.derived_document_text_path(&document_key),
        "stale document text",
    )?;
    fs::write(
        files.derived_page_text_path(&document_key, 1),
        "stale page one",
    )?;
    fs::write(
        files
            .root()
            .join("ocr")
            .join("pages")
            .join(document_key.as_str())
            .join("1.txt"),
        "stale page one",
    )?;
    fs::write(
        files
            .root()
            .join("text")
            .join("pages")
            .join(document_key.as_str())
            .join("99.txt"),
        "orphan page",
    )?;

    Ok(RepairFixture {
        _data_dir: data_dir,
        store,
        files,
        document_key,
    })
}
