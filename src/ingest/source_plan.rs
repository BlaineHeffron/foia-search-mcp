use crate::ingest::pipeline::IngestDocument;
use crate::sources::{CachePolicy, SourceAsset, SourceAssetRole, SourceRecord};
use crate::store::{DocumentKey, StoreError, TextSource};
use serde_json::json;
use std::fmt;

#[derive(Clone, Debug)]
pub struct SourceIngestionPlan {
    pub document: IngestDocument,
    pub asset: PlannedSourceAsset,
    pub cache_policy: CachePolicy,
    pub metadata: SourcePlanMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedSourceAsset {
    pub url: String,
    pub label: String,
    pub mime_type: Option<String>,
    pub role: SourceAssetRole,
    pub text_source: TextSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcePlanMetadata {
    pub source: &'static str,
    pub source_id: String,
    pub cache_policy: &'static str,
    pub selected_asset_role: &'static str,
    pub source_metadata_keys: Vec<String>,
}

#[derive(Debug)]
pub enum SourcePlanError {
    InvalidDocumentKey {
        source: &'static str,
        source_id: String,
        document_key: String,
        error: Box<StoreError>,
    },
    NoIngestibleAsset {
        source: &'static str,
        source_id: String,
        document_url: String,
        asset_roles: Vec<&'static str>,
        guidance: &'static str,
    },
}

impl fmt::Display for SourcePlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDocumentKey {
                source,
                source_id,
                document_key,
                error,
            } => write!(
                f,
                "{source}:{source_id} has invalid document_key {document_key:?}: {error}"
            ),
            Self::NoIngestibleAsset {
                source,
                source_id,
                document_url,
                asset_roles,
                guidance,
            } => write!(
                f,
                "{source}:{source_id} has no ingestible PDF, OCR, transcript, text, or HTML asset at {document_url}; observed asset roles: {}; {guidance}",
                format_roles(asset_roles)
            ),
        }
    }
}

impl std::error::Error for SourcePlanError {}

pub fn plan_source_ingestion(
    record: &SourceRecord,
    cache_policy: CachePolicy,
) -> Result<SourceIngestionPlan, SourcePlanError> {
    let asset = select_asset(record)?;
    let document_key = DocumentKey::new(record.document_key.clone()).map_err(|error| {
        SourcePlanError::InvalidDocumentKey {
            source: record.source,
            source_id: record.source_id.clone(),
            document_key: record.document_key.clone(),
            error: Box::new(error),
        }
    })?;
    let metadata = SourcePlanMetadata {
        source: record.source,
        source_id: record.source_id.clone(),
        cache_policy: cache_policy_name(&cache_policy),
        selected_asset_role: asset_role_name(&asset.role),
        source_metadata_keys: record.metadata.keys().cloned().collect(),
    };
    let metadata_json = plan_metadata_json(&metadata, record, &asset);

    Ok(SourceIngestionPlan {
        document: IngestDocument {
            public_id: record.id.clone(),
            document_key,
            source: record.source.to_owned(),
            source_id: record.source_id.clone(),
            title: record.title.clone(),
            date: record.date.clone(),
            collection: record.collection.clone(),
            record_group: record.record_group.clone(),
            description: record.description.clone(),
            origin_url: Some(record.origin_url.clone()),
            document_url: Some(record.document_url.clone()),
            pdf_url: pdf_url_for_document(record, &asset),
            metadata_json,
            citation_note: record.citation_note.clone(),
            terms_note: record.terms_note.clone(),
            text_source: asset.text_source,
        },
        asset,
        cache_policy,
        metadata,
    })
}

fn select_asset(record: &SourceRecord) -> Result<PlannedSourceAsset, SourcePlanError> {
    record
        .attachments
        .iter()
        .find(|asset| is_pdf_asset(asset))
        .map(plan_pdf_asset)
        .or_else(|| fallback_pdf_asset(record))
        .or_else(|| find_role_asset(record, SourceAssetRole::OcrText))
        .or_else(|| find_role_asset(record, SourceAssetRole::Transcript))
        .or_else(|| find_plain_text_asset(record))
        .or_else(|| find_role_asset(record, SourceAssetRole::Html))
        .ok_or_else(|| no_ingestible_asset(record))
}

fn plan_asset(asset: &SourceAsset) -> PlannedSourceAsset {
    PlannedSourceAsset {
        url: asset.asset_url.clone(),
        label: asset.label.clone(),
        mime_type: asset.mime_type.clone(),
        role: asset.role.clone(),
        text_source: text_source_for_asset(asset),
    }
}

fn plan_pdf_asset(asset: &SourceAsset) -> PlannedSourceAsset {
    PlannedSourceAsset {
        url: asset.asset_url.clone(),
        label: asset.label.clone(),
        mime_type: Some(
            asset
                .mime_type
                .clone()
                .unwrap_or_else(|| "application/pdf".to_owned()),
        ),
        role: SourceAssetRole::Pdf,
        text_source: TextSource::EmbeddedPdfText,
    }
}

fn fallback_pdf_asset(record: &SourceRecord) -> Option<PlannedSourceAsset> {
    let url = record.pdf_url.as_deref()?.trim();
    if url.is_empty() {
        return None;
    }
    Some(PlannedSourceAsset {
        url: url.to_owned(),
        label: "PDF".to_owned(),
        mime_type: Some("application/pdf".to_owned()),
        role: SourceAssetRole::Pdf,
        text_source: TextSource::EmbeddedPdfText,
    })
}

fn find_role_asset(record: &SourceRecord, role: SourceAssetRole) -> Option<PlannedSourceAsset> {
    record
        .attachments
        .iter()
        .find(|asset| !asset.asset_url.trim().is_empty() && asset.role == role)
        .map(plan_asset)
}

fn find_plain_text_asset(record: &SourceRecord) -> Option<PlannedSourceAsset> {
    record
        .attachments
        .iter()
        .find(|asset| !asset.asset_url.trim().is_empty() && is_plain_text_asset(asset))
        .map(|asset| PlannedSourceAsset {
            url: asset.asset_url.clone(),
            label: asset.label.clone(),
            mime_type: asset.mime_type.clone(),
            role: asset.role.clone(),
            text_source: TextSource::ApiText,
        })
}

fn is_pdf_asset(asset: &SourceAsset) -> bool {
    !asset.asset_url.trim().is_empty()
        && (asset.role == SourceAssetRole::Pdf
            || asset
                .mime_type
                .as_deref()
                .is_some_and(|mime| mime.eq_ignore_ascii_case("application/pdf"))
            || url_without_query(&asset.asset_url).ends_with(".pdf"))
}

fn is_plain_text_asset(asset: &SourceAsset) -> bool {
    asset
        .mime_type
        .as_deref()
        .is_some_and(|mime| mime.to_ascii_lowercase().starts_with("text/plain"))
        || url_without_query(&asset.asset_url).ends_with(".txt")
}

fn is_tei_asset(asset: &SourceAsset) -> bool {
    asset.mime_type.as_deref().is_some_and(|mime| {
        let normalized = mime.to_ascii_lowercase();
        normalized == "application/tei+xml" || normalized == "text/tei+xml"
    }) || url_without_query(&asset.asset_url).ends_with(".xml")
}

fn url_without_query(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn text_source_for_asset(asset: &SourceAsset) -> TextSource {
    match asset.role {
        SourceAssetRole::Pdf => TextSource::EmbeddedPdfText,
        SourceAssetRole::OcrText => TextSource::SourceOcr,
        SourceAssetRole::Html => TextSource::Html,
        SourceAssetRole::Transcript => {
            if is_tei_asset(asset) {
                TextSource::Tei
            } else {
                TextSource::ApiText
            }
        }
        SourceAssetRole::Image | SourceAssetRole::Other => {
            if is_plain_text_asset(asset) {
                TextSource::ApiText
            } else {
                TextSource::LocalOcr
            }
        }
    }
}

fn pdf_url_for_document(record: &SourceRecord, asset: &PlannedSourceAsset) -> Option<String> {
    if asset.role == SourceAssetRole::Pdf {
        Some(asset.url.clone())
    } else {
        record.pdf_url.clone()
    }
}

fn plan_metadata_json(
    metadata: &SourcePlanMetadata,
    record: &SourceRecord,
    asset: &PlannedSourceAsset,
) -> String {
    json!({
        "ingest_plan": {
            "source": metadata.source,
            "source_id": metadata.source_id,
            "cache_policy": metadata.cache_policy,
            "selected_asset": {
                "url": asset.url,
                "label": asset.label,
                "mime_type": asset.mime_type,
                "role": metadata.selected_asset_role,
                "text_source": text_source_name(asset.text_source),
            },
            "source_metadata_keys": metadata.source_metadata_keys,
        },
        "source_metadata": record.metadata,
    })
    .to_string()
}

fn no_ingestible_asset(record: &SourceRecord) -> SourcePlanError {
    SourcePlanError::NoIngestibleAsset {
        source: record.source,
        source_id: record.source_id.clone(),
        document_url: record.document_url.clone(),
        asset_roles: record
            .attachments
            .iter()
            .map(|asset| asset_role_name(&asset.role))
            .collect(),
        guidance: "fetch the source record in a browser or update the adapter so it exposes a PDF, OCR text, transcript, plain-text, or HTML asset",
    }
}

fn format_roles(roles: &[&'static str]) -> String {
    if roles.is_empty() {
        "none".to_owned()
    } else {
        roles.join(", ")
    }
}

fn cache_policy_name(policy: &CachePolicy) -> &'static str {
    match policy {
        CachePolicy::RespectSourceHeaders => "respect_source_headers",
        CachePolicy::DoNotPersist => "do_not_persist",
    }
}

fn asset_role_name(role: &SourceAssetRole) -> &'static str {
    match role {
        SourceAssetRole::Pdf => "pdf",
        SourceAssetRole::Html => "html",
        SourceAssetRole::OcrText => "ocr_text",
        SourceAssetRole::Transcript => "transcript",
        SourceAssetRole::Image => "image",
        SourceAssetRole::Other => "other",
    }
}

fn text_source_name(text_source: TextSource) -> &'static str {
    match text_source {
        TextSource::EmbeddedPdfText => "embedded_pdf_text",
        TextSource::SourceOcr => "source_ocr",
        TextSource::LocalOcr => "local_ocr",
        TextSource::Html => "html",
        TextSource::Tei => "tei",
        TextSource::ApiText => "api_text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{SourceAsset, SourceAssetRole, SourceMetadata, SourceRecord};

    fn record(attachments: Vec<SourceAsset>, pdf_url: Option<&str>) -> SourceRecord {
        let mut metadata = SourceMetadata::new();
        metadata.insert(
            "classification".to_owned(),
            "Top Secret Codeword".to_owned(),
        );

        SourceRecord {
            id: "cia:cia-rdp-test".to_owned(),
            document_key: "cia_cia-rdp-test".to_owned(),
            source: "cia",
            source_id: "cia-rdp-test".to_owned(),
            title: "Weather Control".to_owned(),
            date: Some("1967-05-01".to_owned()),
            collection: Some("CREST".to_owned()),
            record_group: None,
            description: Some("A declassified record".to_owned()),
            origin_url: "https://www.cia.gov/readingroom/document/cia-rdp-test".to_owned(),
            document_url: "https://www.cia.gov/readingroom/document/cia-rdp-test".to_owned(),
            pdf_url: pdf_url.map(ToOwned::to_owned),
            metadata,
            attachments,
            text_preview: Some("preview text that should not be copied".to_owned()),
            citation_note: Some("Cite CIA Reading Room record".to_owned()),
            terms_note: Some("Review source terms before reuse".to_owned()),
        }
    }

    fn asset(role: SourceAssetRole, url: &str, mime_type: Option<&str>) -> SourceAsset {
        SourceAsset {
            asset_url: url.to_owned(),
            label: format!("{role:?} asset"),
            mime_type: mime_type.map(ToOwned::to_owned),
            role,
        }
    }

    #[test]
    fn prefers_pdf_over_ocr_and_preserves_notes() {
        let source_record = record(
            vec![
                asset(
                    SourceAssetRole::OcrText,
                    "https://example.test/record.txt",
                    Some("text/plain"),
                ),
                asset(
                    SourceAssetRole::Pdf,
                    "https://example.test/record.pdf",
                    Some("application/pdf"),
                ),
            ],
            None,
        );

        let plan = plan_source_ingestion(&source_record, CachePolicy::RespectSourceHeaders)
            .expect("source record should plan");

        assert_eq!(plan.asset.role, SourceAssetRole::Pdf);
        assert_eq!(plan.asset.text_source, TextSource::EmbeddedPdfText);
        assert_eq!(
            plan.document.pdf_url.as_deref(),
            Some("https://example.test/record.pdf")
        );
        assert_eq!(
            plan.document.citation_note.as_deref(),
            Some("Cite CIA Reading Room record")
        );
        assert_eq!(
            plan.document.terms_note.as_deref(),
            Some("Review source terms before reuse")
        );
        assert_eq!(plan.metadata.cache_policy, "respect_source_headers");
        assert!(plan.document.metadata_json.contains("selected_asset"));
        assert!(plan.document.metadata_json.contains("classification"));
        assert!(plan.document.metadata_json.contains("Top Secret Codeword"));
        assert!(!plan.document.metadata_json.contains("preview text"));
    }

    #[test]
    fn uses_record_pdf_url_when_asset_list_lacks_pdf() {
        let source_record = record(Vec::new(), Some("https://example.test/fallback.pdf"));

        let plan = plan_source_ingestion(&source_record, CachePolicy::DoNotPersist)
            .expect("record pdf_url should be enough to plan");

        assert_eq!(plan.asset.role, SourceAssetRole::Pdf);
        assert_eq!(plan.asset.url, "https://example.test/fallback.pdf");
        assert_eq!(plan.cache_policy, CachePolicy::DoNotPersist);
        assert_eq!(plan.metadata.cache_policy, "do_not_persist");
        assert!(plan
            .document
            .metadata_json
            .contains("\"cache_policy\":\"do_not_persist\""));
    }

    #[test]
    fn falls_back_to_ocr_text_when_no_pdf_exists() {
        let source_record = record(
            vec![asset(
                SourceAssetRole::OcrText,
                "https://example.test/ocr.txt",
                Some("text/plain; charset=utf-8"),
            )],
            None,
        );

        let plan = plan_source_ingestion(&source_record, CachePolicy::DoNotPersist)
            .expect("OCR text should be ingestible");

        assert_eq!(plan.asset.role, SourceAssetRole::OcrText);
        assert_eq!(plan.document.text_source, TextSource::SourceOcr);
        assert_eq!(plan.document.pdf_url, None);
    }

    #[test]
    fn falls_back_to_plain_text_mime_for_other_assets() {
        let source_record = record(
            vec![asset(
                SourceAssetRole::Other,
                "https://example.test/plain.txt",
                Some("text/plain"),
            )],
            None,
        );

        let plan = plan_source_ingestion(&source_record, CachePolicy::DoNotPersist)
            .expect("plain text asset should be ingestible");

        assert_eq!(plan.asset.role, SourceAssetRole::Other);
        assert_eq!(plan.asset.text_source, TextSource::ApiText);
        assert_eq!(plan.document.text_source, TextSource::ApiText);
    }

    #[test]
    fn normalizes_pdf_asset_from_mime_or_url_hint() {
        let source_record = record(
            vec![asset(
                SourceAssetRole::Other,
                "https://example.test/record.pdf?download=1",
                None,
            )],
            None,
        );

        let plan = plan_source_ingestion(&source_record, CachePolicy::RespectSourceHeaders)
            .expect("PDF URL hints should be ingestible");

        assert_eq!(plan.asset.role, SourceAssetRole::Pdf);
        assert_eq!(plan.asset.mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(plan.document.text_source, TextSource::EmbeddedPdfText);
    }

    #[test]
    fn returns_actionable_error_without_ingestible_asset() {
        let source_record = record(
            vec![
                asset(
                    SourceAssetRole::Image,
                    "https://example.test/page.jpg",
                    Some("image/jpeg"),
                ),
                asset(
                    SourceAssetRole::Other,
                    "https://example.test/download.bin",
                    Some("application/octet-stream"),
                ),
            ],
            None,
        );

        let err = plan_source_ingestion(&source_record, CachePolicy::RespectSourceHeaders)
            .expect_err("image and binary assets should not be selected");

        assert!(err.to_string().contains("no ingestible PDF"));
        assert!(err.to_string().contains("image, other"));
        assert!(err.to_string().contains("update the adapter"));
    }

    #[test]
    fn doj_epstein_mixed_media_preserves_sensitive_notes_and_selects_pdf_only() {
        let mut source_record = record(
            vec![
                asset(
                    SourceAssetRole::Image,
                    "https://www.justice.gov/epstein/files/photo.jpg",
                    Some("image/jpeg"),
                ),
                asset(
                    SourceAssetRole::Other,
                    "https://www.justice.gov/epstein/files/video.mp4",
                    Some("video/mp4"),
                ),
                asset(
                    SourceAssetRole::Other,
                    "https://www.justice.gov/epstein/files/audio.mp3",
                    Some("audio/mpeg"),
                ),
                asset(
                    SourceAssetRole::Pdf,
                    "https://www.justice.gov/epstein/files/report.pdf",
                    Some("application/pdf"),
                ),
            ],
            None,
        );
        source_record.source = "doj_epstein";
        source_record.id = "doj_epstein:data-set-1-files".to_owned();
        source_record.source_id = "data-set-1-files".to_owned();
        source_record.metadata.insert(
            "source_warning".to_owned(),
            "DOJ privacy and victim-identification warning".to_owned(),
        );
        source_record.citation_note = Some("Cite official DOJ page/PDF URL.".to_owned());
        source_record.terms_note = Some("Sensitive DOJ Epstein Library content.".to_owned());

        let plan = plan_source_ingestion(&source_record, CachePolicy::RespectSourceHeaders)
            .expect("DOJ record should select ingestible PDF");

        assert_eq!(plan.asset.role, SourceAssetRole::Pdf);
        assert_eq!(
            plan.asset.url,
            "https://www.justice.gov/epstein/files/report.pdf"
        );
        assert!(plan
            .document
            .metadata_json
            .contains("DOJ privacy and victim-identification warning"));
        assert_eq!(
            plan.document.citation_note.as_deref(),
            Some("Cite official DOJ page/PDF URL.")
        );
        assert_eq!(
            plan.document.terms_note.as_deref(),
            Some("Sensitive DOJ Epstein Library content.")
        );
    }

    #[test]
    fn returns_error_for_invalid_document_key() {
        let mut source_record = record(
            vec![asset(
                SourceAssetRole::Pdf,
                "https://example.test/record.pdf",
                Some("application/pdf"),
            )],
            None,
        );
        source_record.document_key = "not a safe/key".to_owned();

        let err = plan_source_ingestion(&source_record, CachePolicy::RespectSourceHeaders)
            .expect_err("invalid source document key should fail planning");

        assert!(matches!(err, SourcePlanError::InvalidDocumentKey { .. }));
        assert!(err.to_string().contains("invalid document_key"));
    }
}
