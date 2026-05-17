use crate::ingest::reconcile::reconcile_derived_artifacts_for_document;
use crate::ingest::reconcile_repair::{DerivedArtifactRepairAction, DerivedArtifactRepairPlan};
use crate::ingest::reconcile_repair_apply::{apply_derived_artifact_repairs, RepairApplyError};
use crate::ingest::reconcile_repair_apply_support_tests::{
    assert_no_temp_files, ocr_page_path, repair_plan, row_count, seed_fixture,
    write_expected_artifacts, write_with_parent,
};
use std::fs;
use std::path::PathBuf;

#[test]
fn apply_rewrites_missing_and_stale_derived_artifacts() {
    let fixture = seed_fixture().expect("seed fixture");

    let stale_document = fixture
        .files
        .derived_document_text_path(&fixture.document_key);
    write_with_parent(&stale_document, "stale document").expect("seed stale document text");

    let stale_page = fixture
        .files
        .derived_page_text_path(&fixture.document_key, 2);
    write_with_parent(&stale_page, "stale page two").expect("seed stale page text");

    let ocr_path = ocr_page_path(&fixture, 2);
    if ocr_path.exists() {
        fs::remove_file(&ocr_path).expect("remove existing ocr fixture");
    }

    let before_documents = row_count(&fixture.store, "documents");
    let before_assets = row_count(&fixture.store, "assets");
    let before_pages = row_count(&fixture.store, "pages");
    let before_chunks = row_count(&fixture.store, "chunks");
    let before_chunk_fts = row_count(&fixture.store, "chunk_fts");

    let plan = repair_plan(&fixture).expect("build repair plan");
    let report = apply_derived_artifact_repairs(&fixture.store, &fixture.files, &plan)
        .expect("apply derived repairs");

    assert!(report.rewritten >= 4);
    assert_eq!(report.skipped_manual_review, 0);
    assert_eq!(report.already_current, 0);

    assert_eq!(
        fs::read_to_string(
            fixture
                .files
                .derived_document_text_path(&fixture.document_key)
        )
        .expect("read repaired document text"),
        "[page 1]\nalpha page one\n\n[page 2]\nbravo page two"
    );
    assert_eq!(
        fs::read_to_string(
            fixture
                .files
                .derived_page_text_path(&fixture.document_key, 1)
        )
        .expect("read repaired page 1"),
        "alpha page one"
    );
    assert_eq!(
        fs::read_to_string(
            fixture
                .files
                .derived_page_text_path(&fixture.document_key, 2)
        )
        .expect("read repaired page 2"),
        "bravo page two"
    );
    assert_eq!(
        fs::read_to_string(ocr_page_path(&fixture, 2)).expect("read repaired ocr page"),
        "bravo page two"
    );

    assert_no_temp_files(fixture.files.root()).expect("check temp file cleanup");

    let post_report = reconcile_derived_artifacts_for_document(
        &fixture.store,
        &fixture.files,
        fixture.document_key.as_str(),
    )
    .expect("post-apply reconcile");
    assert!(post_report.issues.is_empty(), "{:?}", post_report.issues);

    assert_eq!(before_documents, row_count(&fixture.store, "documents"));
    assert_eq!(before_assets, row_count(&fixture.store, "assets"));
    assert_eq!(before_pages, row_count(&fixture.store, "pages"));
    assert_eq!(before_chunks, row_count(&fixture.store, "chunks"));
    assert_eq!(before_chunk_fts, row_count(&fixture.store, "chunk_fts"));
    assert!(fixture.canonical_blob_path.exists());
}

#[test]
fn apply_second_pass_is_idempotent() {
    let fixture = seed_fixture().expect("seed fixture");
    let plan = repair_plan(&fixture).expect("build repair plan");

    let first =
        apply_derived_artifact_repairs(&fixture.store, &fixture.files, &plan).expect("first apply");
    let second = apply_derived_artifact_repairs(&fixture.store, &fixture.files, &plan)
        .expect("second apply");

    assert!(first.rewritten > 0);
    assert_eq!(second.rewritten, 0);
    assert_eq!(
        second.already_current,
        rewrite_action_count(&plan),
        "second apply should treat prior rewrites as already current"
    );
    assert_no_temp_files(fixture.files.root()).expect("check temp file cleanup");
}

#[test]
fn apply_skips_manual_review_without_mutation() {
    let fixture = seed_fixture().expect("seed fixture");
    write_expected_artifacts(&fixture).expect("write expected artifacts");

    let orphan_path = fixture
        .files
        .root()
        .join("ocr")
        .join("pages")
        .join(fixture.document_key.as_str())
        .join("99.txt");
    write_with_parent(&orphan_path, "orphaned ocr text").expect("seed orphan");

    let before_orphan = fs::read_to_string(&orphan_path).expect("read orphan before");
    let before_documents = row_count(&fixture.store, "documents");
    let before_assets = row_count(&fixture.store, "assets");
    let before_pages = row_count(&fixture.store, "pages");

    let plan = repair_plan(&fixture).expect("build repair plan");
    assert!(matches!(
        plan.actions.first(),
        Some(DerivedArtifactRepairAction::ManualReview { .. })
    ));

    let report = apply_derived_artifact_repairs(&fixture.store, &fixture.files, &plan)
        .expect("apply manual-review plan");

    assert_eq!(report.rewritten, 0);
    assert_eq!(report.already_current, 0);
    assert!(report.skipped_manual_review >= 1);
    assert_eq!(
        before_orphan,
        fs::read_to_string(&orphan_path).expect("read orphan after"),
        "manual review should not mutate orphaned file"
    );
    assert_eq!(before_documents, row_count(&fixture.store, "documents"));
    assert_eq!(before_assets, row_count(&fixture.store, "assets"));
    assert_eq!(before_pages, row_count(&fixture.store, "pages"));
    assert!(fixture.canonical_blob_path.exists());
}

#[test]
fn apply_recreates_missing_parent_directories() {
    let fixture = seed_fixture().expect("seed fixture");
    let plan = repair_plan(&fixture).expect("build repair plan");

    let text_root = fixture.files.root().join("text");
    let ocr_root = fixture.files.root().join("ocr");
    if text_root.exists() {
        fs::remove_dir_all(&text_root).expect("remove text root");
    }
    if ocr_root.exists() {
        fs::remove_dir_all(&ocr_root).expect("remove ocr root");
    }

    let report = apply_derived_artifact_repairs(&fixture.store, &fixture.files, &plan)
        .expect("apply with missing parents");
    assert!(report.rewritten > 0);
    assert!(fixture
        .files
        .derived_document_text_path(&fixture.document_key)
        .exists());
    assert!(fixture
        .files
        .derived_page_text_path(&fixture.document_key, 1)
        .exists());
    assert!(ocr_page_path(&fixture, 2).exists());
}

#[test]
fn apply_reports_path_mismatch_explicitly() {
    let fixture = seed_fixture().expect("seed fixture");
    let mut plan = repair_plan(&fixture).expect("build repair plan");

    let first_rewrite = plan
        .actions
        .iter_mut()
        .find_map(|action| match action {
            DerivedArtifactRepairAction::RewriteFromSqlite { path, .. } => Some(path),
            DerivedArtifactRepairAction::ManualReview { .. } => None,
        })
        .expect("rewrite action");
    *first_rewrite = PathBuf::from("/tmp/not-the-canonical-derived-path.txt");

    let error = apply_derived_artifact_repairs(&fixture.store, &fixture.files, &plan)
        .expect_err("path mismatch should fail");

    match error {
        RepairApplyError::InvalidAction { detail } => {
            assert!(detail.contains("does not match canonical path"));
        }
        other => panic!("unexpected error variant: {other}"),
    }
}

fn rewrite_action_count(plan: &DerivedArtifactRepairPlan) -> usize {
    plan.actions
        .iter()
        .filter(|action| {
            matches!(
                action,
                DerivedArtifactRepairAction::RewriteFromSqlite { .. }
            )
        })
        .count()
}
