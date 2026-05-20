use crate::ingest::{plan_source_ingestion, SourcePlanError};
use crate::sources::{CachePolicy, SourceAsset, SourceAssetRole, SourceMetadata, SourceRecord};
use crate::store::TextSource;
use serde_json::Value;

fn mixed_media_record(attachments: Vec<SourceAsset>) -> SourceRecord {
    let mut metadata = SourceMetadata::new();
    metadata.insert(
        "source_collection".to_owned(),
        "mixed-media-release".to_owned(),
    );
    metadata.insert("case_number".to_owned(), "2026-asset-safety".to_owned());

    SourceRecord {
        id: "mixed:test-id/with/pathish/source".to_owned(),
        document_key: "mixed_test_id_with_pathish_source".to_owned(),
        source: "mixed",
        source_id: "test-id/with/pathish/source".to_owned(),
        title: "Mixed media source record".to_owned(),
        date: Some("2026-05-17".to_owned()),
        collection: Some("Representative fixture".to_owned()),
        record_group: None,
        description: Some("A source record with documents and media assets".to_owned()),
        origin_url: "https://example.test/records/test-id".to_owned(),
        document_url: "https://example.test/records/test-id".to_owned(),
        pdf_url: None,
        metadata,
        attachments,
        text_preview: Some("preview should remain metadata-only for planning".to_owned()),
        citation_note: Some("Cite the official source record and selected asset.".to_owned()),
        terms_note: Some("Review source-specific reuse terms.".to_owned()),
    }
}

fn asset(role: SourceAssetRole, url: &str, mime_type: Option<&str>) -> SourceAsset {
    SourceAsset {
        asset_url: url.to_owned(),
        label: format!("{role:?} candidate"),
        mime_type: mime_type.map(ToOwned::to_owned),
        role,
    }
}

fn ingest_plan_json(plan: &crate::ingest::SourceIngestionPlan) -> Value {
    serde_json::from_str(&plan.document.metadata_json).expect("plan metadata should be JSON")
}

#[test]
fn selects_pdf_over_images_audio_video_html_and_other_assets() {
    let record = mixed_media_record(vec![
        asset(
            SourceAssetRole::Image,
            "https://example.test/assets/page-1.jpg",
            Some("image/jpeg"),
        ),
        asset(
            SourceAssetRole::Other,
            "https://example.test/assets/interview.mp3",
            Some("audio/mpeg"),
        ),
        asset(
            SourceAssetRole::Other,
            "https://example.test/assets/clip.mp4",
            Some("video/mp4"),
        ),
        asset(
            SourceAssetRole::Html,
            "https://example.test/assets/landing.html",
            Some("text/html"),
        ),
        asset(
            SourceAssetRole::Other,
            "https://example.test/assets/download.bin",
            Some("application/octet-stream"),
        ),
        asset(
            SourceAssetRole::Pdf,
            "https://example.test/assets/record.pdf",
            Some("application/pdf"),
        ),
    ]);

    let plan = plan_source_ingestion(&record, CachePolicy::RespectSourceHeaders)
        .expect("PDF should be selected from mixed media candidates");
    let metadata = ingest_plan_json(&plan);

    assert_eq!(plan.asset.role, SourceAssetRole::Pdf);
    assert_eq!(plan.asset.url, "https://example.test/assets/record.pdf");
    assert_eq!(plan.asset.mime_type.as_deref(), Some("application/pdf"));
    assert_eq!(plan.asset.text_source, TextSource::EmbeddedPdfText);
    assert_eq!(plan.document.text_source, TextSource::EmbeddedPdfText);
    assert_eq!(
        plan.document.pdf_url.as_deref(),
        Some("https://example.test/assets/record.pdf")
    );
    assert_eq!(plan.cache_policy, CachePolicy::RespectSourceHeaders);
    assert_eq!(plan.metadata.selected_asset_role, "pdf");
    assert_eq!(
        metadata["ingest_plan"]["cache_policy"],
        "respect_source_headers"
    );
    assert_eq!(metadata["ingest_plan"]["selected_asset"]["role"], "pdf");
    assert_eq!(
        metadata["ingest_plan"]["selected_asset"]["text_source"],
        "embedded_pdf_text"
    );
    assert_eq!(
        metadata["source_metadata"]["source_collection"],
        "mixed-media-release"
    );
}

#[test]
fn falls_back_to_html_only_when_no_document_asset_exists() {
    let record = mixed_media_record(vec![
        asset(
            SourceAssetRole::Image,
            "https://example.test/assets/page-1.jpg",
            Some("image/jpeg"),
        ),
        asset(
            SourceAssetRole::Other,
            "https://example.test/assets/interview.mp3",
            Some("audio/mpeg"),
        ),
        asset(
            SourceAssetRole::Html,
            "https://example.test/assets/record.html",
            Some("text/html; charset=utf-8"),
        ),
    ]);

    let plan = plan_source_ingestion(&record, CachePolicy::DoNotPersist)
        .expect("HTML should be selected only when no PDF/text document asset exists");

    assert_eq!(plan.asset.role, SourceAssetRole::Html);
    assert_eq!(plan.asset.text_source, TextSource::Html);
    assert_eq!(plan.document.text_source, TextSource::Html);
    assert_eq!(plan.document.pdf_url, None);
    assert_eq!(plan.cache_policy, CachePolicy::DoNotPersist);
    assert_eq!(plan.metadata.cache_policy, "do_not_persist");
}

#[test]
fn falls_back_to_plain_text_before_html_when_no_pdf_exists() {
    let record = mixed_media_record(vec![
        asset(
            SourceAssetRole::Html,
            "https://example.test/assets/record.html",
            Some("text/html"),
        ),
        asset(
            SourceAssetRole::Other,
            "https://example.test/assets/record.txt",
            Some("text/plain; charset=utf-8"),
        ),
    ]);

    let plan = plan_source_ingestion(&record, CachePolicy::RespectSourceHeaders)
        .expect("plain text should be selected before HTML when no PDF exists");

    assert_eq!(plan.asset.role, SourceAssetRole::Other);
    assert_eq!(plan.asset.url, "https://example.test/assets/record.txt");
    assert_eq!(plan.asset.text_source, TextSource::ApiText);
    assert_eq!(plan.document.text_source, TextSource::ApiText);
}

#[test]
fn reports_actionable_error_with_candidate_roles_for_metadata_only_media() {
    let record = mixed_media_record(vec![
        asset(
            SourceAssetRole::Image,
            "https://example.test/assets/page-1.tif",
            Some("image/tiff"),
        ),
        asset(
            SourceAssetRole::Other,
            "https://example.test/assets/oral-history.wav",
            Some("audio/wav"),
        ),
        asset(
            SourceAssetRole::Other,
            "https://example.test/assets/inspection.mov",
            Some("video/quicktime"),
        ),
    ]);

    let err = plan_source_ingestion(&record, CachePolicy::RespectSourceHeaders)
        .expect_err("media-only candidates should remain metadata-only");

    match err {
        SourcePlanError::NoIngestibleAsset {
            source,
            source_id,
            document_url,
            asset_roles,
            guidance,
        } => {
            assert_eq!(source, "mixed");
            assert_eq!(source_id, "test-id/with/pathish/source");
            assert_eq!(document_url, "https://example.test/records/test-id");
            assert_eq!(asset_roles, vec!["image", "other", "other"]);
            assert!(guidance.contains("update the adapter"));
        }
        other => panic!("expected no-ingestible-asset error, got {other:?}"),
    }
}

#[test]
fn preserves_source_ids_without_promoting_them_to_document_keys() {
    let record = mixed_media_record(vec![asset(
        SourceAssetRole::Pdf,
        "https://example.test/assets/record.pdf",
        Some("application/pdf"),
    )]);

    let plan = plan_source_ingestion(&record, CachePolicy::RespectSourceHeaders)
        .expect("path-like source IDs should plan with a separate safe document key");
    let metadata = ingest_plan_json(&plan);

    assert_eq!(plan.document.public_id, "mixed:test-id/with/pathish/source");
    assert_eq!(plan.document.source_id, "test-id/with/pathish/source");
    assert_eq!(
        plan.document.document_key.as_str(),
        "mixed_test_id_with_pathish_source"
    );
    assert_ne!(plan.document.document_key.as_str(), plan.document.public_id);
    assert_ne!(plan.document.document_key.as_str(), plan.document.source_id);
    assert!(!plan.document.document_key.as_str().contains('/'));
    assert_eq!(
        metadata["ingest_plan"]["source_id"],
        "test-id/with/pathish/source"
    );
}

#[test]
fn govinfo_plan_preserves_notes_warning_policy_and_official_urls() {
    let mut record = mixed_media_record(vec![
        asset(
            SourceAssetRole::Pdf,
            "https://api.govinfo.gov/packages/USREPORTS-99/pdf",
            Some("application/pdf"),
        ),
        asset(
            SourceAssetRole::Other,
            "https://api.govinfo.gov/packages/USREPORTS-99/xml",
            Some("application/xml"),
        ),
    ]);
    record.id = "govinfo:USREPORTS-99".to_owned();
    record.document_key = "govinfo-93c0f3b4c86f328a".to_owned();
    record.source = "govinfo";
    record.source_id = "USREPORTS-99".to_owned();
    record.origin_url = "https://www.govinfo.gov/app/details/USREPORTS-99".to_owned();
    record.document_url = "https://api.govinfo.gov/packages/USREPORTS-99/summary".to_owned();
    record.citation_note = Some("GovInfo citation note".to_owned());
    record.terms_note = Some("GovInfo terms note".to_owned());
    record.metadata.insert(
        "source_warning".to_owned(),
        "GovInfo warning note".to_owned(),
    );
    record.metadata.insert(
        "cache_policy_note".to_owned(),
        "Respect source cache headers".to_owned(),
    );
    record.metadata.insert(
        "redirect_policy_note".to_owned(),
        "Redirects denied by default".to_owned(),
    );

    let plan = plan_source_ingestion(&record, CachePolicy::RespectSourceHeaders)
        .expect("GovInfo source record should produce a PDF-first ingest plan");
    let metadata = ingest_plan_json(&plan);

    assert_eq!(
        plan.document.origin_url.as_deref(),
        Some("https://www.govinfo.gov/app/details/USREPORTS-99")
    );
    assert_eq!(
        plan.document.document_url.as_deref(),
        Some("https://api.govinfo.gov/packages/USREPORTS-99/summary")
    );
    assert_eq!(
        plan.document.citation_note.as_deref(),
        Some("GovInfo citation note")
    );
    assert_eq!(
        plan.document.terms_note.as_deref(),
        Some("GovInfo terms note")
    );
    assert_eq!(
        metadata["source_metadata"]["source_warning"],
        "GovInfo warning note"
    );
    assert_eq!(
        metadata["source_metadata"]["cache_policy_note"],
        "Respect source cache headers"
    );
    assert_eq!(
        metadata["source_metadata"]["redirect_policy_note"],
        "Redirects denied by default"
    );
    assert_eq!(
        metadata["ingest_plan"]["cache_policy"],
        "respect_source_headers"
    );
}
