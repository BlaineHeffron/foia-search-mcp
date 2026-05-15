use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use reqwest::header::{ACCEPT, USER_AGENT};
use serde_json::Value;

use super::{
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
    SourceError, SourceFuture, SourceRecord, SourceStatus,
};

pub const NARA_SOURCE: &str = "nara";
pub const NARA_SEARCH_SOURCE: &str = "nara_catalog";
pub const DEFAULT_BASE_URL: &str = "https://catalog.archives.gov/api/v2";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT_VALUE: &str = "foia-search-mcp/0.1 (+https://github.com/modelcontextprotocol)";
const CITATION_NOTE: &str =
    "National Archives Catalog metadata. Verify digitized object links, OCR, and transcripts at source.";
const TERMS_NOTE: &str =
    "NARA Catalog API use requires an API key and has documented query limits. Persistent API response caching is disabled by default.";
const HTML_WARNING: &str =
    "NARA returned HTML instead of JSON from the configured API endpoint. Check FOIA_SEARCH_NARA_API_BASE_URL or verify the record manually in the Catalog.";

#[derive(Debug, Clone)]
pub struct NaraAdapter {
    base_url: String,
    api_key: Option<String>,
}

impl NaraAdapter {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.and_then(|value| {
                let trimmed = value.trim().to_owned();
                (!trimmed.is_empty()).then_some(trimmed)
            }),
        }
    }

    pub fn from_env() -> Self {
        let base_url = std::env::var("FOIA_SEARCH_NARA_API_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let api_key = std::env::var("FOIA_SEARCH_NARA_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Self::new(base_url, api_key)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn search_url(&self, query: &str, cursor: Option<&str>, limit: usize) -> String {
        let offset = parse_cursor(cursor);
        let page = page_from_offset(offset, limit);
        let mut url = search_endpoint(&self.base_url);
        let separator = if url.contains('?') { '&' } else { '?' };
        url.push(separator);
        url.push_str("q=");
        url.push_str(&percent_encode_query(query));
        url.push_str("&limit=");
        url.push_str(&limit.to_string());
        url.push_str("&page=");
        url.push_str(&page.to_string());
        url.push_str("&availableOnline=true");
        url
    }

    pub fn record_url(&self, id_or_url: &str) -> Result<(String, String), SourceError> {
        let source_id = nara_id_from_url(id_or_url).unwrap_or_else(|| id_or_url.trim().to_owned());
        if source_id.is_empty() {
            return Err(SourceError::invalid_input(
                NARA_SOURCE,
                "NARA record lookup expects a non-empty NAID or Catalog URL.",
                Some(
                    "Pass ids like 595353 or URLs like https://catalog.archives.gov/id/595353."
                        .to_owned(),
                ),
            ));
        }

        let mut url = search_endpoint(&self.base_url);
        let separator = if url.contains('?') { '&' } else { '?' };
        url.push(separator);
        url.push_str("naId=");
        url.push_str(&percent_encode_query(&source_id));
        url.push_str("&limit=1");
        Ok((url, source_id))
    }

    fn require_api_key(&self) -> Result<&str, SourceError> {
        self.api_key.as_deref().ok_or_else(|| {
            SourceError::invalid_input(
                NARA_SOURCE,
                "NARA Catalog API calls require FOIA_SEARCH_NARA_API_KEY.",
                Some(
                    "Set FOIA_SEARCH_NARA_API_KEY to a NARA Catalog API key, then retry the NARA search or record lookup."
                        .to_owned(),
                ),
            )
        })
    }

    async fn fetch_json(&self, url: &str) -> Result<Value, SourceError> {
        let api_key = self.require_api_key()?;
        let response = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|err| SourceError::Fetch {
                source: NARA_SOURCE,
                message: format!("Failed to initialize HTTP client: {err}"),
                url: Some(url.to_owned()),
            })?
            .get(url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT, "application/json")
            .header("x-api-key", api_key)
            .send()
            .await
            .map_err(|err| SourceError::Fetch {
                source: NARA_SOURCE,
                message: format!(
                    "NARA HTTP request failed. Retry later, narrow the query, or verify the Catalog manually. Details: {err}"
                ),
                url: Some(url.to_owned()),
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(SourceError::Fetch {
                source: NARA_SOURCE,
                message: format!(
                    "NARA returned HTTP {status}. Retry later, narrow the query, or verify API key and source limits."
                ),
                url: Some(url.to_owned()),
            });
        }

        let text = response.text().await.map_err(|err| SourceError::Fetch {
            source: NARA_SOURCE,
            message: format!("Failed to read NARA response body: {err}"),
            url: Some(url.to_owned()),
        })?;
        parse_json_text(&text, url)
    }
}

impl Default for NaraAdapter {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL, None)
    }
}

impl SourceAdapter for NaraAdapter {
    fn name(&self) -> &'static str {
        NARA_SOURCE
    }

    fn status(&self) -> SourceStatus {
        if self.api_key.is_some() {
            SourceStatus::Enabled
        } else {
            SourceStatus::Disabled
        }
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move {
            self.require_api_key()?;
            let offset = parse_cursor(options.cursor.as_deref());
            let url = self.search_url(query, options.cursor.as_deref(), options.max_results);
            let payload = self.fetch_json(&url).await?;
            let records = records_from_payload(&payload);
            let mut normalized = records
                .iter()
                .map(record_from_value)
                .collect::<Vec<SourceRecord>>();
            normalized.truncate(options.max_results);
            let total = total_count(&payload);
            let next_offset = offset + normalized.len();
            let next_cursor = if normalized.is_empty() {
                None
            } else if total.map(|total| next_offset < total).unwrap_or(true) {
                Some(make_cursor(next_offset))
            } else {
                None
            };
            let warnings = if normalized.is_empty() {
                vec![
                    "NARA API returned no records. Try broader keywords or remove online-only constraints in a future adapter version.".to_owned(),
                ]
            } else {
                Vec::new()
            };

            Ok(SearchPage {
                query: query.to_owned(),
                source: NARA_SEARCH_SOURCE,
                records: normalized,
                next_cursor,
                warnings,
            })
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            self.require_api_key()?;
            let (url, requested_id) = self.record_url(id_or_url)?;
            let payload = self.fetch_json(&url).await?;
            let Some(record) = records_from_payload(&payload).into_iter().next() else {
                return Err(SourceError::Fetch {
                    source: NARA_SOURCE,
                    message: format!("NARA returned no Catalog record for NAID {requested_id}."),
                    url: Some(url),
                });
            };
            let mut normalized = record_from_value(&record);
            if normalized.source_id == "unknown" {
                normalized.source_id = requested_id;
                normalized.id = format!("{}:{}", NARA_SOURCE, normalized.source_id);
                normalized.document_key = document_key(NARA_SOURCE, &normalized.source_id);
                normalized.origin_url = catalog_url(&normalized.source_id);
                normalized.document_url = normalized.origin_url.clone();
            }
            normalized.text_preview = first_string_for_keys(
                &record,
                &[
                    "scopeAndContentNote",
                    "description",
                    "variantControlNumberNote",
                    "generalNote",
                ],
            );
            Ok(normalized)
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(record.attachments.clone()) })
    }

    fn cache_policy(&self) -> CachePolicy {
        CachePolicy::DoNotPersist
    }
}

fn parse_json_text(text: &str, url: &str) -> Result<Value, SourceError> {
    if text.trim_start().starts_with('<') {
        return Err(SourceError::SourceChanged {
            source: NARA_SOURCE,
            message: HTML_WARNING.to_owned(),
            url: Some(url.to_owned()),
        });
    }
    serde_json::from_str(text).map_err(|err| SourceError::SourceChanged {
        source: NARA_SOURCE,
        message: format!(
            "NARA returned a response that was not valid JSON. Verify the API endpoint and source behavior manually. Details: {err}"
        ),
        url: Some(url.to_owned()),
    })
}

fn records_from_payload(payload: &Value) -> Vec<Value> {
    match payload {
        Value::Object(map) => {
            for key in ["records", "body", "results", "items", "hits"] {
                if let Some(value) = map.get(key) {
                    match value {
                        Value::Array(items) => return object_items(items),
                        Value::Object(_) => {
                            let nested = records_from_payload(value);
                            if !nested.is_empty() {
                                return nested;
                            }
                        }
                        _ => {}
                    }
                }
            }
            map.values()
                .find_map(|value| {
                    let nested = records_from_payload(value);
                    (!nested.is_empty()).then_some(nested)
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn object_items(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .filter_map(|item| {
            if let Some(source) = item.get("_source").filter(|source| source.is_object()) {
                Some(source.clone())
            } else if item.is_object() {
                Some(item.clone())
            } else {
                None
            }
        })
        .collect()
}

fn record_from_value(record: &Value) -> SourceRecord {
    let source_id = first_string_for_keys(record, &["naId", "naIds", "identifier", "id"])
        .unwrap_or_else(|| "unknown".to_owned());
    let title = first_string_for_keys(
        record,
        &[
            "title",
            "description",
            "scopeAndContentNote",
            "objectDescription",
        ],
    )
    .unwrap_or_else(|| source_id.clone());
    let description = first_string_for_keys(
        record,
        &["scopeAndContentNote", "description", "generalNote"],
    );
    let attachments = digital_assets(record);
    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());
    let url = catalog_url(&source_id);

    SourceRecord {
        id: format!("{NARA_SOURCE}:{source_id}"),
        document_key: document_key(NARA_SOURCE, &source_id),
        source: NARA_SOURCE,
        source_id,
        title,
        date: first_string_for_keys(
            record,
            &[
                "date",
                "inclusiveStartDate",
                "productionDate",
                "createdDate",
            ],
        ),
        collection: first_string_for_keys(record, &["collectionIdentifier", "collectionTitle"]),
        record_group: first_string_for_keys(record, &["recordGroup", "recordGroupNumber"]),
        description,
        origin_url: url.clone(),
        document_url: url,
        pdf_url,
        metadata: metadata_from_record(record),
        attachments,
        text_preview: None,
        citation_note: Some(CITATION_NOTE.to_owned()),
        terms_note: Some(TERMS_NOTE.to_owned()),
    }
}

fn metadata_from_record(record: &Value) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    if let Value::Object(map) = record {
        for (key, value) in map {
            if let Some(text) = first_string(value) {
                metadata.insert(key.clone(), text);
            }
        }
    }
    metadata
}

fn digital_assets(record: &Value) -> Vec<SourceAsset> {
    let mut seen = HashSet::new();
    let mut assets = Vec::new();
    collect_urls(record, &mut |url| {
        let lower = url.to_ascii_lowercase();
        let role = if lower.contains(".pdf") {
            SourceAssetRole::Pdf
        } else if lower.contains(".jpg")
            || lower.contains(".jpeg")
            || lower.contains(".png")
            || lower.contains(".gif")
            || lower.contains(".tif")
            || lower.contains(".tiff")
        {
            SourceAssetRole::Image
        } else if lower.contains(".txt") || lower.contains("ocr") {
            SourceAssetRole::OcrText
        } else {
            return;
        };
        if !seen.insert(url.to_owned()) {
            return;
        }
        let mime_type = match role {
            SourceAssetRole::Pdf => Some("application/pdf".to_owned()),
            SourceAssetRole::Image if lower.contains(".png") => Some("image/png".to_owned()),
            SourceAssetRole::Image if lower.contains(".gif") => Some("image/gif".to_owned()),
            SourceAssetRole::Image if lower.contains(".tif") || lower.contains(".tiff") => {
                Some("image/tiff".to_owned())
            }
            SourceAssetRole::Image => Some("image/jpeg".to_owned()),
            SourceAssetRole::OcrText => Some("text/plain".to_owned()),
            _ => None,
        };
        assets.push(SourceAsset {
            label: asset_label(url, &role),
            asset_url: url.to_owned(),
            mime_type,
            role,
        });
    });
    assets
}

fn collect_urls<'a>(value: &'a Value, visit: &mut impl FnMut(&'a str)) {
    match value {
        Value::String(text) if text.starts_with("http://") || text.starts_with("https://") => {
            visit(text)
        }
        Value::Array(items) => {
            for item in items {
                collect_urls(item, visit);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_urls(value, visit);
            }
        }
        _ => {}
    }
}

fn asset_label(url: &str, role: &SourceAssetRole) -> String {
    url.split('/')
        .next_back()
        .and_then(|tail| tail.split(['?', '#']).next())
        .filter(|tail| !tail.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| match role {
            SourceAssetRole::Pdf => "PDF".to_owned(),
            SourceAssetRole::Image => "Digital object image".to_owned(),
            SourceAssetRole::OcrText => "OCR text".to_owned(),
            _ => "Digital object".to_owned(),
        })
}

fn first_string_for_keys(value: &Value, keys: &[&str]) -> Option<String> {
    let Value::Object(map) = value else {
        return None;
    };
    keys.iter()
        .find_map(|key| map.get(*key).and_then(first_string))
}

fn first_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Array(items) => items.iter().find_map(first_string),
        Value::Object(map) => map.values().find_map(first_string),
        _ => None,
    }
}

fn total_count(payload: &Value) -> Option<usize> {
    match payload {
        Value::Object(map) => {
            for key in ["total", "totalRecords", "count", "totalHits"] {
                if let Some(value) = map.get(key).and_then(value_as_usize) {
                    return Some(value);
                }
            }
            map.values().find_map(total_count)
        }
        _ => None,
    }
}

fn value_as_usize(value: &Value) -> Option<usize> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| usize::try_from(value).ok()),
        Value::String(text) => text.parse().ok(),
        Value::Object(map) => map.get("value").and_then(value_as_usize),
        _ => None,
    }
}

pub fn parse_cursor(cursor: Option<&str>) -> usize {
    cursor
        .and_then(|cursor| cursor.strip_prefix("nara-offset-"))
        .and_then(|offset| offset.parse::<usize>().ok())
        .unwrap_or(0)
}

pub fn make_cursor(offset: usize) -> String {
    format!("nara-offset-{offset}")
}

fn page_from_offset(offset: usize, limit: usize) -> usize {
    if limit == 0 {
        1
    } else {
        (offset / limit) + 1
    }
}

fn search_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/records") {
        format!("{base}/search")
    } else if base.ends_with("/records/search") {
        base.to_owned()
    } else {
        format!("{base}/records/search")
    }
}

fn catalog_url(source_id: &str) -> String {
    format!(
        "https://catalog.archives.gov/id/{}",
        percent_encode_query(source_id)
    )
}

fn nara_id_from_url(value: &str) -> Option<String> {
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return None;
    }
    value
        .split("/id/")
        .nth(1)
        .and_then(|tail| tail.split(['?', '#']).next())
        .filter(|id| !id.trim().is_empty())
        .map(|id| id.trim().to_owned())
}

fn document_key(source: &str, source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in source.bytes().chain([b':']).chain(source_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{source}-{hash:016x}")
}

fn percent_encode_query(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records_and_digital_assets() {
        let payload = serde_json::json!({
            "body": {
                "hits": {
                    "total": 2,
                    "records": [{
                        "naId": 595353,
                        "title": "Weather Modification Report",
                        "scopeAndContentNote": "Digitized report.",
                        "recordGroup": "Record Group 59",
                        "digitalObjects": [{
                            "objectUrl": "https://catalog.archives.gov/files/report.pdf"
                        }]
                    }]
                }
            }
        });

        let records = records_from_payload(&payload);
        let record = record_from_value(&records[0]);

        assert_eq!(total_count(&payload), Some(2));
        assert_eq!(record.id, "nara:595353");
        assert_eq!(record.source_id, "595353");
        assert_ne!(record.document_key, record.source_id);
        assert_eq!(
            record.pdf_url.as_deref(),
            Some("https://catalog.archives.gov/files/report.pdf")
        );
    }

    #[test]
    fn parses_elasticsearch_style_hits() {
        let payload = serde_json::json!({
            "body": {
                "hits": {
                    "total": { "value": 1 },
                    "hits": [{
                        "_source": {
                            "naId": "777",
                            "title": "Wrapped Record"
                        }
                    }]
                }
            }
        });

        let records = records_from_payload(&payload);
        let record = record_from_value(&records[0]);

        assert_eq!(total_count(&payload), Some(1));
        assert_eq!(record.source_id, "777");
        assert_eq!(record.title, "Wrapped Record");
    }

    #[test]
    fn cursor_round_trips_offsets() {
        assert_eq!(parse_cursor(Some(&make_cursor(20))), 20);
        assert_eq!(parse_cursor(Some("bad-cursor")), 0);
    }

    #[test]
    fn converts_offsets_to_documented_page_numbers() {
        assert_eq!(page_from_offset(0, 10), 1);
        assert_eq!(page_from_offset(10, 10), 2);
        assert_eq!(page_from_offset(10, 1), 11);
        assert_eq!(page_from_offset(10, 0), 1);
    }
}
