use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod asset;
mod html;
mod parse;
mod url;

use asset::{asset_priority_key, dedupe_assets, is_likely_asset_link};
use parse::{
    record_from_detail_page, record_from_direct_asset_url, record_matches_query,
    records_from_search_page,
};
use url::{canonicalize_official_url, detail_url_from_source_id, is_allowed_state_url};

pub const STATE_SOURCE: &str = "state";
pub const STATE_SEARCH_SOURCE: &str = "state_virtual_reading_room";
pub const STATE_BASE_URL: &str = "https://foia.state.gov";
pub const STATE_SEARCH_PATH: &str = "/Search/Results.aspx";

const SOURCE_CHANGED_WARNING: &str = "State Department FOIA Virtual Reading Room format may have changed. Verify official State FOIA pages manually.";
const CITATION_NOTE: &str = "State Department FOIA Virtual Reading Room official lead. Cite the official State FOIA page and linked PDF URL, and verify PDF page boundaries, OCR quality, redactions, and originating agency before publication.";
const TERMS_NOTE: &str = "Use official foia.state.gov FOIA Library and Virtual Reading Room pages for research leads. Avoid mirrors and bulk scraping; page-level citation requires PDF ingestion and boundary verification.";

#[derive(Debug, Clone)]
pub struct StateAdapter {
    base_url: String,
}

enum StateLocator {
    Url(String),
    SourceId(String),
}

impl StateAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_STATE_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            STATE_BASE_URL.to_owned()
        } else {
            trimmed.trim_end_matches('/').to_owned()
        }
    }

    fn search_url(&self, query: &str) -> String {
        format!(
            "{}{}?searchText={}",
            self.base_url(),
            STATE_SEARCH_PATH,
            url::percent_encode_query(query)
        )
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<StateLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                STATE_SOURCE,
                "State lookup expects a non-empty source id or official State FOIA URL.",
                Some(
                    "Examples: state:FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213 or https://foia.state.gov/FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("state:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            let normalized = canonicalize_official_url(value, &self.base_url());
            if !is_allowed_state_url(&normalized, &self.base_url()) {
                return Err(SourceError::invalid_input(
                    STATE_SOURCE,
                    "State lookup only accepts official same-origin foia.state.gov URLs.",
                    Some(
                        "Use URLs rooted at https://foia.state.gov/Search/ or https://foia.state.gov/FOIALIBRARY/."
                            .to_owned(),
                    ),
                ));
            }
            return Ok(StateLocator::Url(normalized));
        }

        Ok(StateLocator::SourceId(value.to_owned()))
    }

    async fn search_records(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<SearchPage, SourceError> {
        let search_url = self.search_url(query);
        let html = fetch_text(STATE_SOURCE, &search_url).await?;
        let mut records = records_from_search_page(&html, &self.base_url(), &search_url);
        records.retain(|record| record_matches_query(record, query));
        records.truncate(max_results);

        let warnings = if records.is_empty() {
            vec![
                "State Department Virtual Reading Room returned no matching official leads. Try broader terms, a case number, or review foia.state.gov Search Released Documents manually."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: STATE_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        let normalized = canonicalize_official_url(url, &self.base_url());
        if !is_allowed_state_url(&normalized, &self.base_url()) {
            return Err(SourceError::invalid_input(
                STATE_SOURCE,
                "State lookup only accepts official same-origin foia.state.gov URLs.",
                Some("Use official foia.state.gov Search or FOIA Library URLs.".to_owned()),
            ));
        }
        if is_likely_asset_link(&normalized, "") {
            return Ok(record_from_direct_asset_url(&normalized, &self.base_url()));
        }

        let html = fetch_text(STATE_SOURCE, &normalized).await?;
        record_from_detail_page(&html, &self.base_url(), &normalized, None).ok_or_else(|| {
            SourceError::SourceChanged {
                source: STATE_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(normalized),
            }
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let Some(url) = detail_url_from_source_id(source_id, &self.base_url()) else {
            return Err(SourceError::invalid_input(
                STATE_SOURCE,
                "State source_id format is not recognized.",
                Some(
                    "Use ids such as FOIALIBRARY/SearchResults.aspx?caseNumber=F-1990-04213 or state:<official-path>."
                        .to_owned(),
                ),
            ));
        };

        self.get_record_by_url(&url).await
    }
}

impl Default for StateAdapter {
    fn default() -> Self {
        Self::new(STATE_BASE_URL)
    }
}

impl SourceAdapter for StateAdapter {
    fn name(&self) -> &'static str {
        STATE_SOURCE
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
                    STATE_SOURCE,
                    "State FOIA Virtual Reading Room search expects a non-empty query string.",
                    Some(
                        "Try terms such as 'Kissinger', 'Chile', 'cable', or a FOIA case number."
                            .to_owned(),
                    ),
                ));
            }

            self.search_records(query, options.max_results).await
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            match self.parse_locator(id_or_url)? {
                StateLocator::Url(url) => self.get_record_by_url(&url).await,
                StateLocator::SourceId(source_id) => self.get_record_by_source_id(&source_id).await,
            }
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move {
            let mut assets = dedupe_assets(record.attachments.clone());
            assets.sort_by_key(asset_priority_key);
            Ok(assets)
        })
    }
}

pub fn state_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn state_terms_note() -> &'static str {
    TERMS_NOTE
}
