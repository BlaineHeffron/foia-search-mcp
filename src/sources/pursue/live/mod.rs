use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod asset;
mod csv;
mod html;
mod parse;
mod url;

use asset::{ensure_asset_present, single_asset_record};
use parse::{
    dedupe_records, record_from_release_page, record_matches_query, records_from_csv,
    records_from_index_html, release_hint_from_asset_url, release_id_from_url,
};
use url::{absolutize, is_allowed_war_url, percent_encode_path_segment};

pub const PURSUE_SOURCE: &str = "pursue";
pub const PURSUE_SEARCH_SOURCE: &str = "war_gov_ufo";
pub const PURSUE_BASE_URL: &str = "https://www.war.gov";
pub const PURSUE_INDEX_PATH: &str = "/ufo/";

const CITATION_NOTE: &str = "PURSUE official release metadata from war.gov. Verify tranche details, dates, and media context on the official release page before citing.";
const TERMS_NOTE: &str = "Official U.S. Department of War PURSUE release material for unresolved UAP cases. Records may be redacted and include mixed media; prefer official release assets over mirrors or news reposts.";
const SOURCE_CHANGED_WARNING: &str = "PURSUE release page format may have changed. Verify tranche links and assets directly at the official war.gov/UFO page.";

#[derive(Debug, Clone)]
pub struct PursueAdapter {
    base_url: String,
}

#[derive(Debug, Clone)]
enum PursueLocator {
    Release {
        release_id: String,
    },
    Record {
        source_id: String,
    },
    Asset {
        release_id: Option<String>,
        asset_url: String,
    },
    ReleaseArticleUrl {
        release_url: String,
    },
    UfoLanding,
}

impl PursueAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_PURSUE_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn ufo_index_url(&self) -> String {
        absolutize(PURSUE_INDEX_PATH, &self.base_url)
    }

    fn release_url(&self, release_id: &str) -> String {
        if release_id.starts_with("http://") || release_id.starts_with("https://") {
            return release_id.to_owned();
        }

        let encoded = percent_encode_path_segment(release_id);
        format!(
            "{}/ufo/releases/{}/",
            self.base_url.trim_end_matches('/'),
            encoded
        )
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<PursueLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                PURSUE_SOURCE,
                "PURSUE record lookup expects a release id, pursue-prefixed id, official release URL, or official asset URL.",
                Some(
                    "Examples: pursue:release-01, release-01, https://www.war.gov/ufo/, or https://www.war.gov/medialink/ufo/release_1/file.pdf"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("pursue:") {
            value = stripped.trim();
        }

        if value.eq_ignore_ascii_case("ufo") || value.eq_ignore_ascii_case("ufo/") {
            return Ok(PursueLocator::UfoLanding);
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            if !is_allowed_war_url(value, &self.base_url) {
                return Err(SourceError::invalid_input(
                    PURSUE_SOURCE,
                    "PURSUE lookup only accepts same-origin official war.gov release or asset URLs.",
                    Some("Use war.gov/UFO release pages or official linked release assets.".to_owned()),
                ));
            }
            if value
                .to_ascii_lowercase()
                .contains("/news/releases/release/article/")
            {
                return Ok(PursueLocator::ReleaseArticleUrl {
                    release_url: value.to_owned(),
                });
            }
            if value.to_ascii_lowercase().contains("/medialink/ufo/") {
                return Ok(PursueLocator::Asset {
                    release_id: release_hint_from_asset_url(value),
                    asset_url: value.to_owned(),
                });
            }
            if value.to_ascii_lowercase().contains("/ufo") {
                if let Some(release_id) = release_id_from_url(value) {
                    return Ok(PursueLocator::Release { release_id });
                }
                return Ok(PursueLocator::UfoLanding);
            }
            return Err(SourceError::invalid_input(
                PURSUE_SOURCE,
                "Unsupported war.gov URL for PURSUE lookup.",
                Some("Use war.gov/UFO release pages or official linked release assets.".to_owned()),
            ));
        }

        if value.contains(':') && value.to_ascii_lowercase().starts_with("release") {
            return Ok(PursueLocator::Record {
                source_id: value.to_owned(),
            });
        }

        Ok(PursueLocator::Release {
            release_id: parse::normalize_release_id(value),
        })
    }

    async fn parse_index_records(&self) -> Result<Vec<SourceRecord>, SourceError> {
        let index_url = self.ufo_index_url();
        let html = fetch_text(PURSUE_SOURCE, &index_url).await?;

        let mut records = Vec::new();
        for csv_url in parse::csv_links_from_html(&html, &self.base_url) {
            let csv_body = fetch_text(PURSUE_SOURCE, &csv_url).await?;
            records.extend(records_from_csv(&csv_body, &self.base_url));
        }

        if records.is_empty() {
            records.extend(records_from_index_html(&html, &self.base_url));
        }

        Ok(dedupe_records(records))
    }

    async fn release_record_by_id(&self, release_id: &str) -> Result<SourceRecord, SourceError> {
        let release_url = self.release_url(release_id);
        let html = fetch_text(PURSUE_SOURCE, &release_url).await?;
        record_from_release_page(&html, &self.base_url, &release_url, release_id).ok_or_else(|| {
            SourceError::SourceChanged {
                source: PURSUE_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(release_url),
            }
        })
    }

    async fn record_from_index_source_id(
        &self,
        source_id: &str,
    ) -> Result<SourceRecord, SourceError> {
        let records = self.parse_index_records().await?;
        records
            .into_iter()
            .find(|record| record.source_id == source_id || record.id == format!("pursue:{source_id}"))
            .ok_or_else(|| SourceError::Fetch {
                source: PURSUE_SOURCE,
                message: format!(
                    "PURSUE index returned no record for source_id {source_id}. Verify the release tranche on the official war.gov/UFO page."
                ),
                url: Some(self.ufo_index_url()),
            })
    }

    async fn record_from_release_article_url(
        &self,
        release_url: &str,
    ) -> Result<SourceRecord, SourceError> {
        let records = self.parse_index_records().await?;
        let matched = records
            .into_iter()
            .find(|record| urls_match(&record.document_url, release_url));
        if let Some(record) = matched {
            return Ok(record);
        }

        let html = fetch_text(PURSUE_SOURCE, release_url).await?;
        let release_id = parse::release_id_from_article_html(&html)
            .unwrap_or_else(|| "release-unknown".to_owned());
        record_from_release_page(&html, &self.base_url, release_url, &release_id).ok_or_else(|| {
            SourceError::SourceChanged {
                source: PURSUE_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(release_url.to_owned()),
            }
        })
    }

    async fn record_from_explicit_release_url(
        &self,
        release_url: &str,
        release_id_hint: Option<&str>,
    ) -> Result<SourceRecord, SourceError> {
        let html = fetch_text(PURSUE_SOURCE, release_url).await?;
        let release_id = release_id_hint
            .map(parse::normalize_release_id)
            .or_else(|| parse::release_id_from_url(release_url))
            .or_else(|| parse::release_id_from_article_html(&html))
            .unwrap_or_else(|| "release-unknown".to_owned());
        record_from_release_page(&html, &self.base_url, release_url, &release_id).ok_or_else(|| {
            SourceError::SourceChanged {
                source: PURSUE_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(release_url.to_owned()),
            }
        })
    }

    async fn record_from_asset_locator(
        &self,
        release_id: Option<&str>,
        asset_url: &str,
    ) -> Result<SourceRecord, SourceError> {
        if let Some(release_id) = release_id {
            if let Ok(mut record) = self.release_record_by_id(release_id).await {
                ensure_asset_present(&mut record.attachments, asset_url);
                if record.pdf_url.is_none() && asset_url.to_ascii_lowercase().ends_with(".pdf") {
                    record.pdf_url = Some(asset_url.to_owned());
                }
                return Ok(record);
            }
        }

        Ok(single_asset_record(asset_url, &self.base_url))
    }
}

impl Default for PursueAdapter {
    fn default() -> Self {
        Self::new(PURSUE_BASE_URL)
    }
}

impl SourceAdapter for PursueAdapter {
    fn name(&self) -> &'static str {
        PURSUE_SOURCE
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
                    PURSUE_SOURCE,
                    "PURSUE search expects a non-empty query string.",
                    Some(
                        "Try terms such as 'release 01', an agency name, incident location, or UAP document type."
                            .to_owned(),
                    ),
                ));
            }

            let mut records = self.parse_index_records().await?;
            records.retain(|record| record_matches_query(record, query));
            records.truncate(options.max_results);

            let warnings = if records.is_empty() {
                vec![
                    "PURSUE returned no matching tranche records for this query. Try broader terms or open the official war.gov/UFO release page."
                        .to_owned(),
                ]
            } else {
                Vec::new()
            };

            Ok(SearchPage {
                query: query.to_owned(),
                source: PURSUE_SEARCH_SOURCE,
                records,
                next_cursor: None,
                warnings,
            })
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            let locator = self.parse_locator(id_or_url)?;
            match locator {
                PursueLocator::Release { release_id } => {
                    self.release_record_by_id(&release_id).await
                }
                PursueLocator::Record { source_id } => {
                    let record = self.record_from_index_source_id(&source_id).await?;
                    if record
                        .document_url
                        .to_ascii_lowercase()
                        .contains("/news/releases/release/article/")
                    {
                        let release_id_hint = record
                            .metadata
                            .get("release_tranche")
                            .map(String::as_str)
                            .or_else(|| record.source_id.split_once(':').map(|(head, _)| head));
                        if let Ok(mut release_record) = self
                            .record_from_explicit_release_url(&record.document_url, release_id_hint)
                            .await
                        {
                            if let Some(primary_asset) = record.attachments.first() {
                                ensure_asset_present(
                                    &mut release_record.attachments,
                                    &primary_asset.asset_url,
                                );
                            }
                            return Ok(release_record);
                        }
                    }
                    Ok(record)
                }
                PursueLocator::Asset {
                    release_id,
                    asset_url,
                } => {
                    self.record_from_asset_locator(release_id.as_deref(), &asset_url)
                        .await
                }
                PursueLocator::ReleaseArticleUrl { release_url } => {
                    self.record_from_release_article_url(&release_url).await
                }
                PursueLocator::UfoLanding => {
                    let mut records = self.parse_index_records().await?;
                    records.sort_by(|left, right| left.source_id.cmp(&right.source_id));
                    records
                        .into_iter()
                        .next()
                        .ok_or_else(|| SourceError::SourceChanged {
                            source: PURSUE_SOURCE,
                            message: SOURCE_CHANGED_WARNING.to_owned(),
                            url: Some(self.ufo_index_url()),
                        })
                }
            }
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(record.attachments.clone()) })
    }
}

pub fn pursue_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn pursue_terms_note() -> &'static str {
    TERMS_NOTE
}

fn urls_match(left: &str, right: &str) -> bool {
    left.trim_end_matches('/') == right.trim_end_matches('/')
}
