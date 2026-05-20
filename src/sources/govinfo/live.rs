use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

use serde_json::{json, Value};

mod assets;
mod parse;
#[cfg(test)]
mod parse_tests;
mod transport;

use parse::{parse_locator, record_from_search_result, record_from_summary};
use transport::{
    fetch_json_get, fetch_json_post, percent_encode_path_segment, percent_encode_query,
};

pub const GOVINFO_SOURCE: &str = "govinfo";
pub const GOVINFO_SEARCH_SOURCE: &str = "govinfo_search_service";
pub const GOVINFO_API_ROOT: &str = "https://api.govinfo.gov";
pub const GOVINFO_SEARCH_OVERVIEW_URL: &str =
    "https://www.govinfo.gov/features/search-service-overview";

const DEFAULT_GOVINFO_API_KEY: &str = "DEMO_KEY";
const CITATION_NOTE: &str =
    "GovInfo publication metadata. Verify package/granule links and cited pages in the official publication.";
const TERMS_NOTE: &str =
    "Use official GovInfo API search/package/granule endpoints and prefer PDF/XML/MODS links over HTML scraping.";

#[derive(Debug, Clone)]
pub struct GovInfoAdapter {
    api_root: String,
    api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum GovInfoLocator {
    Package {
        package_id: String,
    },
    Granule {
        package_id: String,
        granule_id: String,
    },
}

impl GovInfoAdapter {
    pub fn new(api_root: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            api_root: api_root.into(),
            api_key: api_key.and_then(normalize_optional_string),
        }
    }

    pub fn from_env() -> Self {
        let api_root = std::env::var("FOIA_SEARCH_GOVINFO_API_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| GOVINFO_API_ROOT.to_owned());
        let api_key = std::env::var("FOIA_SEARCH_GOVINFO_API_KEY")
            .ok()
            .and_then(|value| {
                let trimmed = value.trim().to_owned();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            })
            .or_else(|| Some(DEFAULT_GOVINFO_API_KEY.to_owned()));
        Self::new(api_root, api_key)
    }

    pub fn api_root(&self) -> &str {
        &self.api_root
    }

    pub fn search_endpoint(&self) -> String {
        format!("{}/search", self.api_root.trim_end_matches('/'))
    }

    pub(crate) fn package_summary_endpoint(&self, package_id: &str) -> String {
        format!(
            "{}/packages/{}/summary",
            self.api_root.trim_end_matches('/'),
            percent_encode_path_segment(package_id)
        )
    }

    pub(crate) fn granule_summary_endpoint(&self, package_id: &str, granule_id: &str) -> String {
        format!(
            "{}/packages/{}/granules/{}/summary",
            self.api_root.trim_end_matches('/'),
            percent_encode_path_segment(package_id),
            percent_encode_path_segment(granule_id)
        )
    }

    fn require_api_key(&self) -> Result<&str, SourceError> {
        self.api_key.as_deref().ok_or_else(|| {
            SourceError::invalid_input(
                GOVINFO_SOURCE,
                "GovInfo API requests require an API key.",
                Some(
                    "Set FOIA_SEARCH_GOVINFO_API_KEY (or use DEMO_KEY) before calling GovInfo search or record endpoints."
                        .to_owned(),
                ),
            )
        })
    }

    fn api_url_with_key(&self, url: &str) -> Result<String, SourceError> {
        let api_key = self.require_api_key()?;
        let separator = if url.contains('?') { '&' } else { '?' };
        Ok(format!(
            "{url}{separator}api_key={}",
            percent_encode_query(api_key)
        ))
    }

    async fn search_payload(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Value, SourceError> {
        let page_size = options.max_results.clamp(1, 1000);
        let offset_mark = options
            .cursor
            .as_deref()
            .map(str::trim)
            .filter(|cursor| !cursor.is_empty())
            .unwrap_or("*");
        let request_body = json!({
            "query": query,
            "pageSize": page_size,
            "offsetMark": offset_mark,
            "resultLevel": "default"
        });
        let url = self.api_url_with_key(&self.search_endpoint())?;
        fetch_json_post(GOVINFO_SOURCE, &url, &request_body).await
    }

    async fn fetch_record_payload(&self, locator: &GovInfoLocator) -> Result<Value, SourceError> {
        let summary_url = match locator {
            GovInfoLocator::Package { package_id } => self.package_summary_endpoint(package_id),
            GovInfoLocator::Granule {
                package_id,
                granule_id,
            } => self.granule_summary_endpoint(package_id, granule_id),
        };
        let url = self.api_url_with_key(&summary_url)?;
        fetch_json_get(GOVINFO_SOURCE, &url).await
    }
}

impl Default for GovInfoAdapter {
    fn default() -> Self {
        Self::from_env()
    }
}

impl SourceAdapter for GovInfoAdapter {
    fn name(&self) -> &'static str {
        GOVINFO_SOURCE
    }

    fn status(&self) -> SourceStatus {
        SourceStatus::Enabled
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move {
            let query = query.trim();
            let requested_offset_mark = options
                .cursor
                .as_deref()
                .map(str::trim)
                .filter(|cursor| !cursor.is_empty())
                .unwrap_or("*");
            if query.is_empty() {
                return Err(SourceError::invalid_input(
                    GOVINFO_SOURCE,
                    "GovInfo search expects a non-empty query string.",
                    Some(
                        "Provide a focused query and optionally include GovInfo field operators (for example collection, congress, and publishdate)."
                            .to_owned(),
                    ),
                ));
            }

            let payload = self.search_payload(query, &options).await?;
            let results = payload
                .get("results")
                .and_then(Value::as_array)
                .ok_or_else(|| SourceError::SourceChanged {
                    source: GOVINFO_SOURCE,
                    message: "GovInfo search response is missing the expected 'results' array."
                        .to_owned(),
                    url: Some(self.search_endpoint()),
                })?;

            let mut records = Vec::new();
            for result in results {
                if let Some(record) = record_from_search_result(result) {
                    records.push(record);
                }
            }

            let warnings = if records.is_empty() {
                vec![
                    "GovInfo returned no records for this query. Try broader terms, collection filters, or another source."
                        .to_owned(),
                ]
            } else {
                Vec::new()
            };

            let next_cursor = payload
                .get("offsetMark")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|cursor| !cursor.is_empty())
                .filter(|cursor| !records.is_empty() && *cursor != requested_offset_mark)
                .map(ToOwned::to_owned);

            Ok(SearchPage {
                query: query.to_owned(),
                source: GOVINFO_SEARCH_SOURCE,
                records,
                next_cursor,
                warnings,
            })
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            let locator = parse_locator(id_or_url)?;
            let payload = self.fetch_record_payload(&locator).await?;
            record_from_summary(&payload, &locator)
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(record.attachments.clone()) })
    }
}

pub fn govinfo_terms_note() -> &'static str {
    TERMS_NOTE
}

pub fn govinfo_citation_note() -> &'static str {
    CITATION_NOTE
}

fn normalize_optional_string(value: String) -> Option<String> {
    let trimmed = value.trim().to_owned();
    (!trimmed.is_empty()).then_some(trimmed)
}
