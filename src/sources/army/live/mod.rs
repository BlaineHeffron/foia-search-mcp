use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod asset;
mod html;
mod parse;
mod url;

use asset::{asset_priority_key, dedupe_assets, is_direct_download_url};
use parse::{
    record_from_detail_page, record_from_direct_asset_url, record_matches_query,
    records_from_reading_room_page,
};
use url::{canonicalize_official_url, detail_url_from_source_id, is_allowed_army_url};

pub const ARMY_SOURCE: &str = "army";
pub const ARMY_SEARCH_SOURCE: &str = "army_foia_reading_room";
pub const ARMY_BASE_URL: &str = "https://foia.army.mil";
pub const ARMY_READING_ROOM_PATH: &str = "/";

const SOURCE_CHANGED_WARNING: &str = "Army FOIA Reading Room format may have changed. Verify official foia.army.mil FOIA Reading Room pages manually.";
const CITATION_NOTE: &str = "Official Army FOIA Reading Room lead from foia.army.mil. Cite the official Army FOIA page and linked document URL, and verify PDF page boundaries, OCR quality, redactions, and originating context before publication.";
const TERMS_NOTE: &str = "Use official foia.army.mil Army FOIA Reading Room pages for research leads. Avoid mirrors and bulk scraping; page-level citation requires PDF ingestion and boundary verification.";

const SEARCH_PATHS: &[&str] = &[
    "/",
    "/Home/publicRecords/78",
    "/Home/publicRecords/94",
    "/Home/publicRecords/93",
];

#[derive(Debug, Clone)]
pub struct ArmyAdapter {
    base_url: String,
}

enum ArmyLocator {
    Url(String),
    SourceId(String),
}

impl ArmyAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_ARMY_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            ARMY_BASE_URL.to_owned()
        } else {
            trimmed.trim_end_matches('/').to_owned()
        }
    }

    fn search_urls(&self) -> Vec<String> {
        SEARCH_PATHS
            .iter()
            .map(|path| format!("{}{}", self.base_url(), path))
            .collect()
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<ArmyLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                ARMY_SOURCE,
                "Army FOIA lookup expects a non-empty source id or official Army FOIA URL.",
                Some(
                    "Examples: army:Home/DocContent/4743 or https://foia.army.mil/Home/publicRecords/78"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("army:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            let normalized = canonicalize_official_url(value, &self.base_url());
            if !is_allowed_army_url(&normalized, &self.base_url()) {
                return Err(SourceError::invalid_input(
                    ARMY_SOURCE,
                    "Army FOIA lookup only accepts official same-origin foia.army.mil URLs.",
                    Some("Use URLs rooted at https://foia.army.mil/Home/.".to_owned()),
                ));
            }
            return Ok(ArmyLocator::Url(normalized));
        }

        Ok(ArmyLocator::SourceId(value.to_owned()))
    }

    async fn search_records(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<SearchPage, SourceError> {
        let mut records = Vec::new();
        for url in self.search_urls() {
            let html = fetch_text(ARMY_SOURCE, &url).await?;
            records.extend(records_from_reading_room_page(
                &html,
                &self.base_url(),
                &url,
            ));
        }
        records.retain(|record| record_matches_query(record, query));
        records.truncate(max_results);

        let warnings = if records.is_empty() {
            vec![
                "Army FOIA Reading Room returned no matching official leads. Try broader terms or review the official foia.army.mil Reading Room manually."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: ARMY_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        let normalized = canonicalize_official_url(url, &self.base_url());
        if !is_allowed_army_url(&normalized, &self.base_url()) {
            return Err(SourceError::invalid_input(
                ARMY_SOURCE,
                "Army FOIA lookup only accepts official same-origin foia.army.mil URLs.",
                Some("Use official foia.army.mil Army FOIA Reading Room URLs.".to_owned()),
            ));
        }
        if is_direct_download_url(&normalized) {
            return Ok(record_from_direct_asset_url(&normalized, &self.base_url()));
        }

        let html = fetch_text(ARMY_SOURCE, &normalized).await?;
        record_from_detail_page(&html, &self.base_url(), &normalized, None).ok_or_else(|| {
            SourceError::SourceChanged {
                source: ARMY_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(normalized),
            }
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let Some(url) = detail_url_from_source_id(source_id, &self.base_url()) else {
            return Err(SourceError::invalid_input(
                ARMY_SOURCE,
                "Army source_id format is not recognized.",
                Some("Use ids such as Home/DocContent/4743 or army:<official-path>.".to_owned()),
            ));
        };

        self.get_record_by_url(&url).await
    }
}

impl Default for ArmyAdapter {
    fn default() -> Self {
        Self::new(ARMY_BASE_URL)
    }
}

impl SourceAdapter for ArmyAdapter {
    fn name(&self) -> &'static str {
        ARMY_SOURCE
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
                    ARMY_SOURCE,
                    "Army FOIA Reading Room search expects a non-empty query string.",
                    Some(
                        "Try terms such as 'IG report', 'FOIA log', 'Manning', or 'Agent Orange'."
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
                ArmyLocator::Url(url) => self.get_record_by_url(&url).await,
                ArmyLocator::SourceId(source_id) => self.get_record_by_source_id(&source_id).await,
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

pub fn army_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn army_terms_note() -> &'static str {
    TERMS_NOTE
}
