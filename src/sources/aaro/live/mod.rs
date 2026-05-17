use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod asset;
mod direct;
mod html;
mod parse;
mod url;

use asset::{asset_priority_key, dedupe_assets, is_likely_asset_link};
use direct::record_from_direct_asset_url;
use parse::{record_from_detail_page, record_matches_query, records_from_listing_page};
use url::{
    canonicalize_official_url, detail_url_from_source_id, is_allowed_aaro_url, source_id_from_url,
};

pub const AARO_SOURCE: &str = "aaro";
pub const AARO_SEARCH_SOURCE: &str = "aaro_uap_records";
pub const AARO_BASE_URL: &str = "https://www.aaro.mil";
pub const AARO_RECORDS_PATH: &str = "/UAP-Records/";

const SOURCE_CHANGED_WARNING: &str =
    "AARO records format may have changed. Verify official AARO UAP Records and case-resolution pages manually.";
const CITATION_NOTE: &str = "AARO official UAP historical records and case releases. Cite the official AARO page and linked PDF URL, and verify release context, redactions, and page boundaries before publication.";
const TERMS_NOTE: &str = "Use official AARO-hosted or AARO-linked government records for UAP historical research. Avoid mirrors or news reposts, and avoid bulk scraping beyond source guidance.";

#[derive(Debug, Clone)]
pub struct AaroAdapter {
    base_url: String,
}

enum AaroLocator {
    Url(String),
    SourceId(String),
}

impl AaroAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_AARO_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            AARO_BASE_URL.to_owned()
        } else {
            trimmed.trim_end_matches('/').to_owned()
        }
    }

    fn records_url(&self) -> String {
        format!("{}{}", self.base_url(), AARO_RECORDS_PATH)
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<AaroLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                AARO_SOURCE,
                "AARO lookup expects a source id or official AARO URL.",
                Some(
                    "Examples: aaro:UAP-Records/history-and-origin-of-kona-blue, UAP-Cases/UAP-Case-Resolution-Reports/, or https://www.aaro.mil/UAP-Records/"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("aaro:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            let normalized = canonicalize_official_url(value, &self.base_url());
            if !is_allowed_aaro_url(&normalized, &self.base_url()) {
                return Err(SourceError::invalid_input(
                    AARO_SOURCE,
                    "AARO lookup only accepts official same-origin AARO URLs.",
                    Some(
                        "Use URLs rooted at https://www.aaro.mil from AARO records or case-resolution pages."
                            .to_owned(),
                    ),
                ));
            }
            return Ok(AaroLocator::Url(normalized));
        }

        Ok(AaroLocator::SourceId(value.to_owned()))
    }

    async fn search_records(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<SearchPage, SourceError> {
        let records_url = self.records_url();
        let html = fetch_text(AARO_SOURCE, &records_url).await?;

        let mut records = records_from_listing_page(&html, &self.base_url(), &records_url);
        records.retain(|record| record_matches_query(record, query));
        records.truncate(max_results);

        let warnings = if records.is_empty() {
            vec![
                "AARO records returned no matching leads. Try broader terms (for example, 'KONA BLUE', 'case resolution', or 'UAP records') or review official AARO records pages manually."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: AARO_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        let normalized = canonicalize_official_url(url, &self.base_url());
        if !is_allowed_aaro_url(&normalized, &self.base_url()) {
            return Err(SourceError::invalid_input(
                AARO_SOURCE,
                "AARO lookup only accepts official same-origin AARO URLs.",
                Some("Use URLs rooted at https://www.aaro.mil/...".to_owned()),
            ));
        }
        if is_likely_asset_link(&normalized, "") {
            return Ok(record_from_direct_asset_url(
                &normalized,
                None,
                &self.base_url(),
            ));
        }

        let html = fetch_text(AARO_SOURCE, &normalized).await?;
        record_from_detail_page(&html, &self.base_url(), &normalized, None).ok_or_else(|| {
            SourceError::SourceChanged {
                source: AARO_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(normalized),
            }
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let normalized_source_id = source_id.trim().trim_matches('/').to_owned();
        let Some(detail_url) = detail_url_from_source_id(&normalized_source_id, &self.base_url())
        else {
            return Err(SourceError::invalid_input(
                AARO_SOURCE,
                "AARO source_id format is not recognized.",
                Some(
                    "Use ids such as UAP-Records/history-and-origin-of-kona-blue, UAP-Cases/UAP-Case-Resolution-Reports, or aaro:<path>."
                        .to_owned(),
                ),
            ));
        };

        if is_likely_asset_link(&detail_url, "") {
            return Ok(record_from_direct_asset_url(
                &detail_url,
                None,
                &self.base_url(),
            ));
        }

        let html = fetch_text(AARO_SOURCE, &detail_url).await?;
        record_from_detail_page(
            &html,
            &self.base_url(),
            &detail_url,
            Some(&source_id_from_url(&detail_url)),
        )
        .ok_or_else(|| SourceError::SourceChanged {
            source: AARO_SOURCE,
            message: SOURCE_CHANGED_WARNING.to_owned(),
            url: Some(detail_url),
        })
    }
}

impl Default for AaroAdapter {
    fn default() -> Self {
        Self::new(AARO_BASE_URL)
    }
}

impl SourceAdapter for AaroAdapter {
    fn name(&self) -> &'static str {
        AARO_SOURCE
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
                    AARO_SOURCE,
                    "AARO search expects a non-empty query string.",
                    Some(
                        "Try terms such as 'KONA BLUE', 'case resolution', 'UAP records', or an agency name (DHS/NARA/NASA)."
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
                AaroLocator::Url(url) => self.get_record_by_url(&url).await,
                AaroLocator::SourceId(source_id) => self.get_record_by_source_id(&source_id).await,
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

pub fn aaro_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn aaro_terms_note() -> &'static str {
    TERMS_NOTE
}
