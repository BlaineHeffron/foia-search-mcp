use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct PlaceholderResponse {
    pub status: &'static str,
    pub tool: &'static str,
    pub message: String,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceRecord {
    pub id: String,
    pub document_key: Option<String>,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub date: Option<String>,
    pub collection: Option<String>,
    pub record_group: Option<String>,
    pub description: Option<String>,
    pub origin_url: Option<String>,
    pub document_url: Option<String>,
    pub pdf_url: Option<String>,
    pub assets: Vec<SourceAsset>,
    pub metadata_json: Value,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
    pub source_warning: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAsset {
    pub asset_url: String,
    pub label: String,
    pub mime_type: Option<String>,
    pub role: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchPage {
    pub source: String,
    pub query: String,
    pub records: Vec<SourceRecord>,
    pub next_cursor: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngestionJob {
    pub id: String,
    pub status: String,
    pub document_id: Option<String>,
    pub progress: f32,
    pub next_actions: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalDocument {
    pub id: String,
    pub document_key: String,
    pub public_id: String,
    pub title: String,
    pub source: String,
    pub source_id: String,
    pub date: Option<String>,
    pub collection: Option<String>,
    pub record_group: Option<String>,
    pub description: Option<String>,
    pub origin_url: Option<String>,
    pub document_url: Option<String>,
    pub pdf_url: Option<String>,
    pub metadata_json: Value,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
    pub source_warning: Option<String>,
    pub page_count: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalDocumentText {
    pub document_key: String,
    pub public_id: String,
    pub title: String,
    pub page_start: u32,
    pub page_end: u32,
    pub pages: Vec<LocalPageText>,
    pub text: String,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
    pub source_warning: Option<String>,
    pub warnings: Vec<String>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalPageText {
    pub page_number: u32,
    pub citation: String,
    pub text_source: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalSearchHit {
    pub document_key: String,
    pub chunk_id: String,
    pub source: String,
    pub title: String,
    pub page_start: i64,
    pub page_end: i64,
    pub score: f64,
    pub snippet: String,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
    pub source_warning: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DerivedArtifactRepairIssue {
    pub kind: String,
    pub issue: String,
    pub path: String,
    pub page_number: Option<u32>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DerivedArtifactRepairReportResponse {
    pub document_id: String,
    pub document_key: String,
    pub issue_count: usize,
    pub issues: Vec<DerivedArtifactRepairIssue>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DerivedArtifactRepairAction {
    pub kind: String,
    pub action: String,
    pub path: String,
    pub page_number: Option<u32>,
    pub reason: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DerivedArtifactRepairPlanResponse {
    pub document_id: String,
    pub document_key: String,
    pub action_count: usize,
    pub rewrite_count: usize,
    pub manual_review_count: usize,
    pub actions: Vec<DerivedArtifactRepairAction>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DerivedArtifactRepairApplyResponse {
    pub document_id: String,
    pub document_key: String,
    pub issue_count: usize,
    pub rewritten: usize,
    pub already_current: usize,
    pub skipped_manual_review: usize,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SqliteFtsRepairIssue {
    pub issue: String,
    pub document_key: String,
    pub chunk_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SqliteFtsRepairReportResponse {
    pub canonical_chunk_count: usize,
    pub chunk_fts_row_count: usize,
    pub issue_count: usize,
    pub issues: Vec<SqliteFtsRepairIssue>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SqliteFtsRepairAction {
    pub action: String,
    pub document_key: String,
    pub chunk_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SqliteFtsRepairPlanResponse {
    pub action_count: usize,
    pub rewrite_count: usize,
    pub manual_review_count: usize,
    pub actions: Vec<SqliteFtsRepairAction>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SqliteFtsRepairApplyResponse {
    pub issue_count: usize,
    pub rewritten: usize,
    pub already_current: usize,
    pub skipped_manual_review: usize,
    pub next_actions: Vec<String>,
}

impl From<crate::sources::SourceRecord> for SourceRecord {
    fn from(record: crate::sources::SourceRecord) -> Self {
        let metadata_json = serde_json::to_value(record.metadata).unwrap_or(Value::Null);
        let source_warning = source_warning_from_metadata(&metadata_json);
        let warnings = source_warning.iter().cloned().collect();
        Self {
            id: record.id,
            document_key: Some(record.document_key),
            source: record.source.to_owned(),
            source_id: record.source_id,
            title: record.title,
            date: record.date,
            collection: record.collection,
            record_group: record.record_group,
            description: record.description,
            origin_url: Some(record.origin_url),
            document_url: Some(record.document_url),
            pdf_url: record.pdf_url,
            assets: record
                .attachments
                .into_iter()
                .map(SourceAsset::from)
                .collect(),
            metadata_json,
            citation_note: record.citation_note,
            terms_note: record.terms_note,
            source_warning,
            warnings,
        }
    }
}

impl From<crate::sources::SourceAsset> for SourceAsset {
    fn from(asset: crate::sources::SourceAsset) -> Self {
        Self {
            asset_url: asset.asset_url,
            label: asset.label,
            mime_type: asset.mime_type,
            role: source_asset_role_name(&asset.role).to_owned(),
        }
    }
}

pub(crate) fn source_warning_from_metadata(metadata: &Value) -> Option<String> {
    metadata
        .get("source_metadata")
        .and_then(|source_metadata| source_metadata.get("source_warning"))
        .or_else(|| metadata.get("source_warning"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|warning| !warning.is_empty())
        .map(str::to_owned)
}

fn source_asset_role_name(role: &crate::sources::SourceAssetRole) -> &'static str {
    match role {
        crate::sources::SourceAssetRole::Pdf => "pdf",
        crate::sources::SourceAssetRole::Html => "html",
        crate::sources::SourceAssetRole::OcrText => "ocr_text",
        crate::sources::SourceAssetRole::Transcript => "transcript",
        crate::sources::SourceAssetRole::Image => "image",
        crate::sources::SourceAssetRole::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use crate::sources::{SourceAssetRole, SourceMetadata};

    use super::*;

    #[test]
    fn source_record_response_promotes_warning_and_assets() {
        let mut metadata = SourceMetadata::new();
        metadata.insert(
            "source_warning".to_owned(),
            "DOJ privacy and victim-identification warning".to_owned(),
        );

        let response = SourceRecord::from(crate::sources::SourceRecord {
            id: "doj_epstein:data-set-1-files".to_owned(),
            document_key: "doc_doj_epstein_data_set_1_files".to_owned(),
            source: "doj_epstein",
            source_id: "data-set-1-files".to_owned(),
            title: "DOJ Epstein data set 1 files".to_owned(),
            date: None,
            collection: Some("DOJ Epstein Library".to_owned()),
            record_group: Some("efta_data_set".to_owned()),
            description: None,
            origin_url: "https://www.justice.gov/epstein/doj-disclosures".to_owned(),
            document_url: "https://www.justice.gov/epstein/doj-disclosures/data-set-1-files"
                .to_owned(),
            pdf_url: Some("https://www.justice.gov/epstein/files/report.pdf".to_owned()),
            metadata,
            attachments: vec![
                asset(
                    SourceAssetRole::Pdf,
                    "https://www.justice.gov/epstein/files/report.pdf",
                    Some("application/pdf"),
                ),
                asset(
                    SourceAssetRole::Image,
                    "https://www.justice.gov/epstein/files/page.jpg",
                    Some("image/jpeg"),
                ),
                asset(
                    SourceAssetRole::Other,
                    "https://www.justice.gov/epstein/files/video.mp4",
                    Some("video/mp4"),
                ),
            ],
            text_preview: None,
            citation_note: Some("Cite official DOJ page/PDF URL.".to_owned()),
            terms_note: Some("Sensitive DOJ Epstein Library content.".to_owned()),
        });

        assert_eq!(
            response.source_warning.as_deref(),
            Some("DOJ privacy and victim-identification warning")
        );
        assert_eq!(
            response.warnings,
            vec!["DOJ privacy and victim-identification warning".to_owned()]
        );
        assert_eq!(response.assets[0].role, "pdf");
        assert_eq!(response.assets[1].role, "image");
        assert_eq!(response.assets[2].mime_type.as_deref(), Some("video/mp4"));
        assert_eq!(
            response.metadata_json["source_warning"],
            "DOJ privacy and victim-identification warning"
        );
    }

    #[test]
    fn source_warning_helper_accepts_persisted_source_metadata_shape() {
        let metadata = serde_json::json!({
            "source_metadata": {
                "source_warning": " DOJ persisted warning "
            }
        });

        assert_eq!(
            source_warning_from_metadata(&metadata).as_deref(),
            Some("DOJ persisted warning")
        );
    }

    fn asset(
        role: SourceAssetRole,
        asset_url: &str,
        mime_type: Option<&str>,
    ) -> crate::sources::SourceAsset {
        crate::sources::SourceAsset {
            asset_url: asset_url.to_owned(),
            label: format!("{role:?} asset"),
            mime_type: mime_type.map(str::to_owned),
            role,
        }
    }
}
