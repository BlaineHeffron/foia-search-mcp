use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod assets;
mod html;
mod parse;
mod url;

use assets::dedupe_and_sort_assets;
use parse::{record_from_detail_html, records_from_search_html};
use url::{detail_endpoint, parse_locator, search_endpoint, NoaaLocator};

pub const NOAA_SOURCE: &str = "noaa";
pub const NOAA_SEARCH_SOURCE: &str = "noaa_repository_search";
pub const NOAA_BASE_URL: &str = "https://repository.library.noaa.gov";

const CITATION_NOTE: &str = "NOAA Institutional Repository metadata. Cite the official repository item URL and linked publication PDF, and verify report/program identifiers before publication.";
const TERMS_NOTE: &str = "Use official NOAA Institutional Repository pages and assets. Prefer repository metadata plus official publication PDFs, preserve item-level rights/license statements, and treat non-official mirrors as out-of-scope for citation.";

#[derive(Debug, Clone)]
pub struct NoaaAdapter {
    base_url: String,
}

impl NoaaAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_NOAA_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> &str {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            NOAA_BASE_URL
        } else {
            trimmed
        }
    }

    async fn search_repository(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<SearchPage, SourceError> {
        let endpoint = search_endpoint(self.base_url(), query, options.cursor.as_deref());
        let body = fetch_text(NOAA_SOURCE, &endpoint).await?;

        let mut records = records_from_search_html(&body, self.base_url(), &endpoint)?;
        records.retain(|record| record_matches_query(record, query));
        records.truncate(options.max_results.min(50));

        let warnings = if records.is_empty() {
            vec![
                "NOAA Institutional Repository returned no matching records. Try broader terms, NOAA office names, report numbers, DOI fragments, or collection keywords."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: NOAA_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let endpoint = detail_endpoint(self.base_url(), source_id);
        let body = fetch_text(NOAA_SOURCE, &endpoint).await?;
        record_from_detail_html(&body, self.base_url(), &endpoint, Some(source_id))
    }
}

impl Default for NoaaAdapter {
    fn default() -> Self {
        Self::new(NOAA_BASE_URL)
    }
}

impl SourceAdapter for NoaaAdapter {
    fn name(&self) -> &'static str {
        NOAA_SOURCE
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
            if query.is_empty() {
                return Err(SourceError::invalid_input(
                    NOAA_SOURCE,
                    "NOAA repository search expects a non-empty query string.",
                    Some(
                        "Try publication titles, NOAA offices/programs, report numbers, DOI fragments, or technical topics."
                            .to_owned(),
                    ),
                ));
            }

            self.search_repository(query, &options).await
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            match parse_locator(id_or_url)? {
                NoaaLocator::SourceId(source_id) => self.get_record_by_source_id(&source_id).await,
                NoaaLocator::OfficialUrl(source_id) => {
                    self.get_record_by_source_id(&source_id).await
                }
            }
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(dedupe_and_sort_assets(record.attachments.clone())) })
    }
}

pub fn noaa_terms_note() -> &'static str {
    TERMS_NOTE
}

pub fn noaa_citation_note() -> &'static str {
    CITATION_NOTE
}

fn record_matches_query(record: &SourceRecord, query: &str) -> bool {
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return true;
    }

    let mut haystack = vec![
        record.title.as_str(),
        record.source_id.as_str(),
        record.document_url.as_str(),
    ];
    if let Some(description) = record.description.as_deref() {
        haystack.push(description);
    }
    if let Some(collection) = record.collection.as_deref() {
        haystack.push(collection);
    }
    if let Some(group) = record.record_group.as_deref() {
        haystack.push(group);
    }
    for value in record.metadata.values() {
        haystack.push(value.as_str());
    }

    let normalized_haystack = haystack.join(" ").to_ascii_lowercase();
    normalized_query
        .split_whitespace()
        .all(|token| normalized_haystack.contains(token))
}
