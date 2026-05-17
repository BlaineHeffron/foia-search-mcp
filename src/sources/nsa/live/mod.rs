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
    records_from_listing_page,
};
use url::{canonicalize_official_url, detail_url_from_source_id, is_allowed_nsa_url};

pub const NSA_SOURCE: &str = "nsa";
pub const NSA_SEARCH_SOURCE: &str = "nsa_foia_reading_room";
pub const NSA_BASE_URL: &str = "https://www.nsa.gov";
pub const NSA_READING_ROOM_PATH: &str = "/Helpful-Links/NSA-FOIA/Reading-Room/";
pub const NSA_REPORTS_LIST_PATH: &str = "/Helpful-Links/NSA-FOIA/Declassification-Transparency-Initiatives/FOIA-Reports-and-Releases/FOIA-Reports-and-Releases-List/";

const SOURCE_CHANGED_WARNING: &str =
    "NSA FOIA Reading Room format may have changed. Verify official NSA FOIA pages manually.";
const CITATION_NOTE: &str = "NSA FOIA Reading Room official lead. Cite the official NSA page and linked PDF URL, and verify PDF page boundaries, redactions, and release context before publication.";
const TERMS_NOTE: &str = "Use official NSA FOIA Reading Room and FOIA Reports and Releases pages for research leads. Avoid mirrors and bulk scraping; page-level citation requires PDF ingestion and boundary verification.";

#[derive(Debug, Clone)]
pub struct NsaAdapter {
    base_url: String,
}

enum NsaLocator {
    Url(String),
    SourceId(String),
}

impl NsaAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_NSA_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            NSA_BASE_URL.to_owned()
        } else {
            trimmed.trim_end_matches('/').to_owned()
        }
    }

    fn reading_room_url(&self) -> String {
        format!("{}{}", self.base_url(), NSA_READING_ROOM_PATH)
    }

    fn reports_list_url(&self) -> String {
        format!("{}{}", self.base_url(), NSA_REPORTS_LIST_PATH)
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<NsaLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                NSA_SOURCE,
                "NSA lookup expects a non-empty source id or official NSA FOIA URL.",
                Some(
                    "Examples: nsa:Helpful-Links/NSA-FOIA/Reading-Room/FOIA-Handbook, Helpful-Links/NSA-FOIA/Declassification-Transparency-Initiatives/FOIA-Reports-and-Releases/FOIA-Reports-and-Releases-List/igphoto/2003550765, or https://www.nsa.gov/Helpful-Links/NSA-FOIA/Reading-Room/"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("nsa:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            let normalized = canonicalize_official_url(value, &self.base_url());
            if !is_allowed_nsa_url(&normalized, &self.base_url()) {
                return Err(SourceError::invalid_input(
                    NSA_SOURCE,
                    "NSA lookup only accepts official same-origin NSA FOIA URLs.",
                    Some(
                        "Use URLs rooted at https://www.nsa.gov/Helpful-Links/NSA-FOIA/ or official NSA PDF paths."
                            .to_owned(),
                    ),
                ));
            }
            return Ok(NsaLocator::Url(normalized));
        }

        Ok(NsaLocator::SourceId(value.to_owned()))
    }

    async fn search_records(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<SearchPage, SourceError> {
        let reading_room_url = self.reading_room_url();
        let reports_list_url = self.reports_list_url();
        let reading_room_html = fetch_text(NSA_SOURCE, &reading_room_url).await?;
        let reports_html = fetch_text(NSA_SOURCE, &reports_list_url).await?;

        let mut records =
            records_from_listing_page(&reading_room_html, &self.base_url(), &reading_room_url);
        records.extend(records_from_listing_page(
            &reports_html,
            &self.base_url(),
            &reports_list_url,
        ));
        records.retain(|record| record_matches_query(record, query));
        records.truncate(max_results);

        let warnings = if records.is_empty() {
            vec![
                "NSA FOIA Reading Room returned no matching leads from the official Reading Room and FOIA Reports and Releases pages. Try broader terms or review official NSA FOIA pages manually."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: NSA_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        let normalized = canonicalize_official_url(url, &self.base_url());
        if !is_allowed_nsa_url(&normalized, &self.base_url()) {
            return Err(SourceError::invalid_input(
                NSA_SOURCE,
                "NSA lookup only accepts official same-origin NSA FOIA URLs.",
                Some(
                    "Use URLs rooted at https://www.nsa.gov/Helpful-Links/NSA-FOIA/...".to_owned(),
                ),
            ));
        }
        if is_likely_asset_link(&normalized, "") {
            return Ok(record_from_direct_asset_url(&normalized, &self.base_url()));
        }

        let html = fetch_text(NSA_SOURCE, &normalized).await?;
        record_from_detail_page(&html, &self.base_url(), &normalized, None).ok_or_else(|| {
            SourceError::SourceChanged {
                source: NSA_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(normalized),
            }
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let Some(url) = detail_url_from_source_id(source_id, &self.base_url()) else {
            return Err(SourceError::invalid_input(
                NSA_SOURCE,
                "NSA source_id format is not recognized.",
                Some(
                    "Use ids such as Helpful-Links/NSA-FOIA/Reading-Room/FOIA-Handbook or nsa:<official-path>."
                        .to_owned(),
                ),
            ));
        };

        self.get_record_by_url(&url).await
    }
}

impl Default for NsaAdapter {
    fn default() -> Self {
        Self::new(NSA_BASE_URL)
    }
}

impl SourceAdapter for NsaAdapter {
    fn name(&self) -> &'static str {
        NSA_SOURCE
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
                    NSA_SOURCE,
                    "NSA FOIA Reading Room search expects a non-empty query string.",
                    Some(
                        "Try terms such as 'FOIA Handbook', 'Roswell', 'Inspector General', or 'Cryptologic Almanac'."
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
                NsaLocator::Url(url) => self.get_record_by_url(&url).await,
                NsaLocator::SourceId(source_id) => self.get_record_by_source_id(&source_id).await,
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

pub fn nsa_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn nsa_terms_note() -> &'static str {
    TERMS_NOTE
}
