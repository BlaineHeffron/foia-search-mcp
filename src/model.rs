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
    pub metadata_json: Value,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
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
            metadata_json,
            citation_note: record.citation_note,
            terms_note: record.terms_note,
        }
    }
}
