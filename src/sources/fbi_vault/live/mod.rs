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
    record_from_detail_page, record_matches_query, records_from_search_page, single_asset_record,
};
use url::{
    canonicalize_official_url, detail_url_from_source_id, is_allowed_vault_url,
    percent_encode_path_segment,
};

pub const FBI_VAULT_SOURCE: &str = "fbi_vault";
pub const FBI_VAULT_SEARCH_SOURCE: &str = "fbi_vault_search";
pub const FBI_VAULT_BASE_URL: &str = "https://vault.fbi.gov";

const SOURCE_CHANGED_WARNING: &str =
    "FBI Vault page format may have changed. Verify the official Vault page and linked files manually.";
const CITATION_NOTE: &str = "FBI Vault metadata. Cite the official FBI Vault page and PDF URL, and verify multipart part ordering, page boundaries, and redactions before publication.";
const TERMS_NOTE: &str = "FBI Vault files include FOIA proactive disclosures and frequently requested records, with historically uneven multipart layouts. Use official Vault pages and avoid bulk scraping beyond source guidance.";

#[derive(Debug, Clone)]
pub struct FbiVaultAdapter {
    base_url: String,
}

enum FbiVaultLocator {
    Url(String),
    SourceId(String),
}

impl FbiVaultAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_FBI_VAULT_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            FBI_VAULT_BASE_URL.to_owned()
        } else {
            trimmed.trim_end_matches('/').to_owned()
        }
    }

    fn search_url(&self, query: &str) -> String {
        let encoded = query
            .split_whitespace()
            .map(percent_encode_path_segment)
            .collect::<Vec<_>>()
            .join("+");
        format!("{}/search?SearchableText={encoded}", self.base_url())
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<FbiVaultLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                FBI_VAULT_SOURCE,
                "FBI Vault lookup expects a non-empty source id or official vault URL.",
                Some(
                    "Examples: fbi_vault:rosenberg-case/mark-page, rosenberg-case/mark-page, https://vault.fbi.gov/rosenberg-case/mark-page, or https://vault.fbi.gov/rosenberg-case/mark-page/Mark%20Page%20Part%2001/at_download/file"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("fbi_vault:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            if !is_allowed_vault_url(value, &self.base_url()) {
                return Err(SourceError::invalid_input(
                    FBI_VAULT_SOURCE,
                    "FBI Vault lookup only accepts official vault.fbi.gov URLs.",
                    Some(
                        "Use URLs rooted at https://vault.fbi.gov/... from search_source results or official Vault pages."
                            .to_owned(),
                    ),
                ));
            }
            return Ok(FbiVaultLocator::Url(canonicalize_official_url(
                value,
                &self.base_url(),
            )));
        }

        Ok(FbiVaultLocator::SourceId(value.to_owned()))
    }

    async fn search_records(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<SearchPage, SourceError> {
        let search_url = self.search_url(query);
        let html = fetch_text(FBI_VAULT_SOURCE, &search_url).await?;
        let mut records = records_from_search_page(&html, &self.base_url());
        records.retain(|record| record_matches_query(record, query));
        records.truncate(max_results);

        let warnings = if records.is_empty() {
            vec![
                "FBI Vault search returned no matching records. Try broader subject terms or explicit file names (for example, 'UFO Part 01')."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: FBI_VAULT_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        let normalized = canonicalize_official_url(url, &self.base_url());
        if !is_allowed_vault_url(&normalized, &self.base_url()) {
            return Err(SourceError::invalid_input(
                FBI_VAULT_SOURCE,
                "FBI Vault lookup only accepts official vault.fbi.gov URLs.",
                Some("Use URLs rooted at https://vault.fbi.gov/...".to_owned()),
            ));
        }

        if normalized
            .to_ascii_lowercase()
            .ends_with("/at_download/file")
        {
            return Ok(single_asset_record(&normalized, &self.base_url()));
        }

        let html = fetch_text(FBI_VAULT_SOURCE, &normalized).await?;
        record_from_detail_page(&html, &self.base_url(), &normalized, None).ok_or_else(|| {
            SourceError::SourceChanged {
                source: FBI_VAULT_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(normalized),
            }
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let Some(url) = detail_url_from_source_id(source_id, &self.base_url()) else {
            return Err(SourceError::invalid_input(
                FBI_VAULT_SOURCE,
                "FBI Vault source_id format is not recognized.",
                Some(
                    "Use ids such as rosenberg-case/mark-page, fbi-vault-topic, or fbi_vault:<slug-path>."
                        .to_owned(),
                ),
            ));
        };

        self.get_record_by_url(&url).await
    }
}

impl Default for FbiVaultAdapter {
    fn default() -> Self {
        Self::new(FBI_VAULT_BASE_URL)
    }
}

impl SourceAdapter for FbiVaultAdapter {
    fn name(&self) -> &'static str {
        FBI_VAULT_SOURCE
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
                    FBI_VAULT_SOURCE,
                    "FBI Vault search expects a non-empty query string.",
                    Some(
                        "Try terms such as 'ufo', 'part 01', 'proactive disclosure', or a specific file/subject name."
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
                FbiVaultLocator::Url(url) => self.get_record_by_url(&url).await,
                FbiVaultLocator::SourceId(source_id) => {
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

pub fn fbi_vault_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn fbi_vault_terms_note() -> &'static str {
    TERMS_NOTE
}
