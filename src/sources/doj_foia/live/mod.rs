use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod asset;
mod classify;
mod html;
mod parse;
mod url;

use asset::{asset_priority_key, dedupe_assets};
use parse::{
    record_from_component_page, record_matches_query, records_from_index_page, single_asset_record,
};
use url::{canonicalize_official_url, is_allowed_component_url};

pub const DOJ_FOIA_SOURCE: &str = "doj_foia";
pub const DOJ_FOIA_SEARCH_SOURCE: &str = "doj_component_foia_index";
pub const DOJ_FOIA_COMPONENT_INDEX_URL: &str =
    "https://www.justice.gov/oip/available-documents-all-doj-components";

const CITATION_NOTE: &str = "DOJ component FOIA/disclosure library metadata. Cite the official DOJ component page or PDF URL and verify publication context, date, and redactions before publication.";
const TERMS_NOTE: &str = "DOJ component proactive disclosure and FOIA library content. Respect DOJ/component terms and context, and avoid bulk scraping outside official source guidance.";
const SOURCE_CHANGED_WARNING: &str = "DOJ component FOIA/disclosure page format may have changed. Verify the official DOJ OIP index and component page manually.";

#[derive(Debug, Clone)]
pub struct DojFoiaAdapter {
    index_url: String,
}

enum DojFoiaLocator {
    Url(String),
    SourceId(String),
}

impl DojFoiaAdapter {
    pub fn new(index_url: impl Into<String>) -> Self {
        Self {
            index_url: index_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_DOJ_FOIA_INDEX_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn index_url(&self) -> String {
        let trimmed = self.index_url.trim();
        if trimmed.is_empty() {
            DOJ_FOIA_COMPONENT_INDEX_URL.to_owned()
        } else {
            trimmed.trim_end_matches('/').to_owned()
        }
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<DojFoiaLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                DOJ_FOIA_SOURCE,
                "DOJ FOIA lookup expects a non-empty source id or official component URL.",
                Some(
                    "Examples: doj_foia:criminal-division, criminal-division, https://www.justice.gov/criminal/foia/foia-reading-room-records, or https://www.justice.gov/criminal/foia/docs/2014annual-letter-final-072814.pdf".to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("doj_foia:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            if !is_allowed_component_url(value, &self.index_url()) {
                return Err(SourceError::invalid_input(
                    DOJ_FOIA_SOURCE,
                    "DOJ FOIA lookup only accepts official DOJ component FOIA/disclosure URLs.",
                    Some(
                        "Use URLs from justice.gov/usdoj.gov component FOIA pages or official component domains listed in the DOJ OIP all-components index."
                            .to_owned(),
                    ),
                ));
            }
            return Ok(DojFoiaLocator::Url(canonicalize_official_url(value)));
        }

        Ok(DojFoiaLocator::SourceId(value.to_owned()))
    }

    async fn index_records(&self) -> Result<Vec<SourceRecord>, SourceError> {
        let index_url = self.index_url();
        let html = fetch_text(DOJ_FOIA_SOURCE, &index_url).await?;
        Ok(records_from_index_page(&html, &index_url))
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let records = self.index_records().await?;
        let Some(listing_record) = records.into_iter().find(|record| {
            record.source_id == source_id || record.id == format!("doj_foia:{source_id}")
        }) else {
            return Err(SourceError::invalid_input(
                DOJ_FOIA_SOURCE,
                format!(
                    "No DOJ component lead matched source_id '{source_id}' in the OIP all-components index."
                ),
                Some("Call search_source with source 'doj_foia' first, then pass one returned source_id to get_source_record.".to_owned()),
            ));
        };

        self.get_record_by_url_with_hint(
            &listing_record.document_url,
            listing_record
                .metadata
                .get("component_name")
                .map(String::as_str),
        )
        .await
    }

    async fn get_record_by_url_with_hint(
        &self,
        url: &str,
        component_hint: Option<&str>,
    ) -> Result<SourceRecord, SourceError> {
        if !is_allowed_component_url(url, &self.index_url()) {
            return Err(SourceError::invalid_input(
                DOJ_FOIA_SOURCE,
                "DOJ FOIA lookup only accepts official DOJ component FOIA/disclosure URLs.",
                Some(
                    "Use URLs from justice.gov/usdoj.gov component FOIA pages or official component domains listed in the DOJ OIP all-components index."
                        .to_owned(),
                ),
            ));
        }

        let url = canonicalize_official_url(url);
        let lower = url.to_ascii_lowercase();
        if lower.ends_with(".pdf") || lower.contains(".pdf?") {
            return Ok(single_asset_record(&url, &self.index_url(), component_hint));
        }

        let html = fetch_text(DOJ_FOIA_SOURCE, &url).await?;
        record_from_component_page(&html, &self.index_url(), &url, component_hint).ok_or_else(
            || SourceError::SourceChanged {
                source: DOJ_FOIA_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(url),
            },
        )
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        self.get_record_by_url_with_hint(url, None).await
    }
}

impl Default for DojFoiaAdapter {
    fn default() -> Self {
        Self::new(DOJ_FOIA_COMPONENT_INDEX_URL)
    }
}

impl SourceAdapter for DojFoiaAdapter {
    fn name(&self) -> &'static str {
        DOJ_FOIA_SOURCE
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
                    DOJ_FOIA_SOURCE,
                    "DOJ FOIA search expects a non-empty query string.",
                    Some(
                        "Try component-focused terms such as 'criminal division', 'civil rights', 'foia library', or 'request log'."
                            .to_owned(),
                    ),
                ));
            }

            let mut records = self.index_records().await?;
            records.retain(|record| record_matches_query(record, query));
            records.truncate(options.max_results);

            let warnings = if records.is_empty() {
                vec![
                    "DOJ component FOIA search returned no matching component leads. Try broader component names or 'foia library' terms."
                        .to_owned(),
                ]
            } else {
                Vec::new()
            };

            Ok(SearchPage {
                query: query.to_owned(),
                source: DOJ_FOIA_SEARCH_SOURCE,
                records,
                next_cursor: None,
                warnings,
            })
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            match self.parse_locator(id_or_url)? {
                DojFoiaLocator::Url(url) => self.get_record_by_url(&url).await,
                DojFoiaLocator::SourceId(source_id) => {
                    self.get_record_by_source_id(&source_id).await
                }
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

pub fn doj_foia_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn doj_foia_terms_note() -> &'static str {
    TERMS_NOTE
}
