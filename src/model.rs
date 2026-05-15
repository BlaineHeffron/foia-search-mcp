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
    pub title: String,
    pub source: String,
    pub source_id: String,
    pub page_count: Option<u32>,
    pub warnings: Vec<String>,
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
