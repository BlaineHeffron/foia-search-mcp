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
    records_from_reading_room_page,
};
use url::{canonicalize_official_url, detail_url_from_source_id, is_allowed_dia_url};

pub const DIA_SOURCE: &str = "dia";
pub const DIA_SEARCH_SOURCE: &str = "dia_foia_electronic_reading_room";
pub const DIA_BASE_URL: &str = "https://www.dia.mil";
pub const DIA_READING_ROOM_PATH: &str = "/FOIA/FOIA-Electronic-Reading-Room/";

const SOURCE_CHANGED_WARNING: &str = "DIA FOIA Electronic Reading Room format may have changed. Verify official DIA FOIA pages manually.";
const CITATION_NOTE: &str = "DIA FOIA Electronic Reading Room official lead. Cite the official DIA page and linked PDF URL, and verify PDF page boundaries, OCR quality, redactions, and originating context before publication.";
const TERMS_NOTE: &str = "Use official dia.mil FOIA Electronic Reading Room pages for research leads. Avoid mirrors and bulk scraping; page-level citation requires PDF ingestion and boundary verification.";

#[derive(Debug, Clone)]
pub struct DiaAdapter {
    base_url: String,
}

enum DiaLocator {
    Url(String),
    SourceId(String),
}

impl DiaAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_DIA_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            DIA_BASE_URL.to_owned()
        } else {
            trimmed.trim_end_matches('/').to_owned()
        }
    }

    fn reading_room_url(&self) -> String {
        format!("{}{}", self.base_url(), DIA_READING_ROOM_PATH)
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<DiaLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                DIA_SOURCE,
                "DIA lookup expects a non-empty source id or official DIA FOIA URL.",
                Some(
                    "Examples: dia:FOIA/FOIA-Electronic-Reading-Room/FileId/162286 or https://www.dia.mil/FOIA/FOIA-Electronic-Reading-Room/"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("dia:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            let normalized = canonicalize_official_url(value, &self.base_url());
            if !is_allowed_dia_url(&normalized, &self.base_url()) {
                return Err(SourceError::invalid_input(
                    DIA_SOURCE,
                    "DIA lookup only accepts official same-origin www.dia.mil FOIA URLs.",
                    Some("Use URLs rooted at https://www.dia.mil/FOIA/.".to_owned()),
                ));
            }
            return Ok(DiaLocator::Url(normalized));
        }

        Ok(DiaLocator::SourceId(value.to_owned()))
    }

    async fn search_records(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<SearchPage, SourceError> {
        let reading_room_url = self.reading_room_url();
        let html = fetch_text(DIA_SOURCE, &reading_room_url).await?;
        let mut records =
            records_from_reading_room_page(&html, &self.base_url(), &reading_room_url);
        records.retain(|record| record_matches_query(record, query));
        records.truncate(max_results);

        let warnings = if records.is_empty() {
            vec![
                "DIA FOIA Electronic Reading Room returned no matching official leads. Try broader terms or review the official dia.mil FOIA Electronic Reading Room manually."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: DIA_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        let normalized = canonicalize_official_url(url, &self.base_url());
        if !is_allowed_dia_url(&normalized, &self.base_url()) {
            return Err(SourceError::invalid_input(
                DIA_SOURCE,
                "DIA lookup only accepts official same-origin www.dia.mil FOIA URLs.",
                Some("Use official dia.mil FOIA Electronic Reading Room URLs.".to_owned()),
            ));
        }
        if is_likely_asset_link(&normalized, "") {
            return Ok(record_from_direct_asset_url(&normalized, &self.base_url()));
        }

        let html = fetch_text(DIA_SOURCE, &normalized).await?;
        record_from_detail_page(&html, &self.base_url(), &normalized, None).ok_or_else(|| {
            SourceError::SourceChanged {
                source: DIA_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(normalized),
            }
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let Some(url) = detail_url_from_source_id(source_id, &self.base_url()) else {
            return Err(SourceError::invalid_input(
                DIA_SOURCE,
                "DIA source_id format is not recognized.",
                Some(
                    "Use ids such as FOIA/FOIA-Electronic-Reading-Room/FileId/162286 or dia:<official-path>."
                        .to_owned(),
                ),
            ));
        };

        self.get_record_by_url(&url).await
    }
}

impl Default for DiaAdapter {
    fn default() -> Self {
        Self::new(DIA_BASE_URL)
    }
}

impl SourceAdapter for DiaAdapter {
    fn name(&self) -> &'static str {
        DIA_SOURCE
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
                    DIA_SOURCE,
                    "DIA FOIA Electronic Reading Room search expects a non-empty query string.",
                    Some("Try terms such as 'Argentina', 'terrorism', 'UFO', or 'biological warfare'.".to_owned()),
                ));
            }

            self.search_records(query, options.max_results).await
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            match self.parse_locator(id_or_url)? {
                DiaLocator::Url(url) => self.get_record_by_url(&url).await,
                DiaLocator::SourceId(source_id) => self.get_record_by_source_id(&source_id).await,
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

pub fn dia_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn dia_terms_note() -> &'static str {
    TERMS_NOTE
}
