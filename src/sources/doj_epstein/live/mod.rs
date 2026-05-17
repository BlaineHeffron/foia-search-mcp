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

use asset::{asset_from_url, asset_priority_key, dedupe_assets, media_type_for_asset};
use classify::record_matches_query;
use parse::{
    detail_url_from_source_id, record_from_detail_page, records_from_disclosures_page,
    sensitive_warning, source_id_from_url,
};
use url::{document_key, is_allowed_justice_epstein_url};

pub const DOJ_EPSTEIN_SOURCE: &str = "doj_epstein";
pub const DOJ_EPSTEIN_SOURCE_SEARCH: &str = "doj_epstein_library";
pub const DOJ_EPSTEIN_BASE_URL: &str = "https://www.justice.gov/epstein";
pub const DOJ_EPSTEIN_DISCLOSURES_PATH: &str = "/epstein/doj-disclosures";
pub const DOJ_EPSTEIN_DISCLOSURES_URL: &str = "https://www.justice.gov/epstein/doj-disclosures";

const SOURCE_CHANGED_WARNING: &str = "DOJ Epstein Library page format may have changed. Verify the official DOJ disclosure page and linked records manually.";
const CITATION_NOTE: &str = "DOJ Epstein Library records. Cite the official DOJ page/PDF URL and verify redactions, context, and page boundaries on the DOJ source page before publication.";
const TERMS_NOTE: &str = "Sensitive DOJ Epstein Library content may include victim-identification risks and sexual-assault material. Redactions are applied by DOJ; treat non-PDF media as metadata-only until media safety rules are reviewed.";

#[derive(Debug, Clone)]
pub struct DojEpsteinAdapter {
    base_url: String,
}

enum DojEpsteinLocator {
    Url(String),
    SourceId(String),
}

impl DojEpsteinAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_DOJ_EPSTEIN_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn disclosures_url(&self) -> String {
        format!("{}{}", self.base_url_origin(), DOJ_EPSTEIN_DISCLOSURES_PATH)
    }

    fn epstein_home_url(&self) -> String {
        format!("{}/epstein", self.base_url_origin())
    }

    fn base_url_origin(&self) -> String {
        let trimmed = self.base_url.trim_end_matches('/');
        if trimmed.ends_with("/epstein") {
            return trimmed
                .trim_end_matches('/')
                .trim_end_matches("/epstein")
                .to_owned();
        }
        if let Some((origin, _rest)) = trimmed.split_once("/epstein") {
            return origin.to_owned();
        }
        trimmed.to_owned()
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<DojEpsteinLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                DOJ_EPSTEIN_SOURCE,
                "DOJ Epstein lookup expects a non-empty source id or official justice.gov URL.",
                Some(
                    "Examples: doj_epstein:data-set-1-files, data-set-1-files, https://www.justice.gov/epstein, or https://www.justice.gov/epstein/doj-disclosures/foia-customs-and-border-protection-cbp"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("doj_epstein:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            if !is_allowed_justice_epstein_url(value, &self.base_url) {
                return Err(SourceError::invalid_input(
                    DOJ_EPSTEIN_SOURCE,
                    "DOJ Epstein lookup only accepts official justice.gov/epstein or justice.gov/media URLs.",
                    Some(
                        "Use URLs rooted at https://www.justice.gov/epstein or official linked justice.gov/media assets."
                            .to_owned(),
                    ),
                ));
            }
            return Ok(DojEpsteinLocator::Url(value.to_owned()));
        }

        Ok(DojEpsteinLocator::SourceId(value.to_owned()))
    }

    async fn search_records(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<SearchPage, SourceError> {
        let disclosures_url = self.disclosures_url();
        let html = fetch_text(DOJ_EPSTEIN_SOURCE, &disclosures_url).await?;
        let mut records = records_from_disclosures_page(&html, &self.base_url);
        records.retain(|record| record_matches_query(record, query));
        records.truncate(max_results);

        let mut warnings = vec![sensitive_warning().to_owned()];
        if records.is_empty() {
            warnings.push(
                "DOJ Epstein search returned no matching records. Try broader terms such as 'data set', 'court records', 'foia', or a case name."
                    .to_owned(),
            );
        }

        Ok(SearchPage {
            query: query.to_owned(),
            source: DOJ_EPSTEIN_SOURCE_SEARCH,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        if url.to_ascii_lowercase().contains("/media/") {
            return Ok(self.single_asset_record(url));
        }

        let html = fetch_text(DOJ_EPSTEIN_SOURCE, url).await?;
        record_from_detail_page(&html, &self.base_url, url, None).ok_or_else(|| {
            SourceError::SourceChanged {
                source: DOJ_EPSTEIN_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(url.to_owned()),
            }
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let disclosures_url = self.disclosures_url();
        let epstein_home_url = self.epstein_home_url();
        let Some(url) = detail_url_from_source_id(source_id, &epstein_home_url, &disclosures_url)
        else {
            return Err(SourceError::invalid_input(
                DOJ_EPSTEIN_SOURCE,
                "DOJ Epstein source_id format is not recognized.",
                Some(
                    "Use ids such as data-set-1-files, court-records-..., foia-..., bop-video-footage, or doj_epstein:<source_id>."
                        .to_owned(),
                ),
            ));
        };

        self.get_record_by_url(&url).await
    }

    fn single_asset_record(&self, asset_url: &str) -> SourceRecord {
        let asset = asset_from_url(asset_url);
        let source_id = source_id_from_url(asset_url);
        let category = "release";
        let mut metadata = crate::sources::SourceMetadata::new();
        metadata.insert("library_section".to_owned(), "doj_disclosures".to_owned());
        metadata.insert("category".to_owned(), category.to_owned());
        metadata.insert("official_url".to_owned(), asset_url.to_owned());
        metadata.insert("source_warning".to_owned(), sensitive_warning().to_owned());
        metadata.insert("asset_filename".to_owned(), asset.label.clone());
        metadata.insert(
            "media_type".to_owned(),
            media_type_for_asset(&asset).to_owned(),
        );

        SourceRecord {
            id: format!("{DOJ_EPSTEIN_SOURCE}:{source_id}"),
            document_key: document_key(DOJ_EPSTEIN_SOURCE, &source_id),
            source: DOJ_EPSTEIN_SOURCE,
            source_id,
            title: format!("DOJ Epstein asset {}", asset.label),
            date: None,
            collection: Some("DOJ Epstein Library".to_owned()),
            record_group: Some(category.to_owned()),
            description: Some(
                "Direct DOJ Epstein asset URL. Prefer PDF assets for ingestion and treat non-PDF media as metadata-only."
                    .to_owned(),
            ),
            origin_url: self.disclosures_url(),
            document_url: asset_url.to_owned(),
            pdf_url: (asset.role == crate::sources::SourceAssetRole::Pdf)
                .then(|| asset.asset_url.clone()),
            metadata,
            attachments: vec![asset],
            text_preview: None,
            citation_note: Some(CITATION_NOTE.to_owned()),
            terms_note: Some(TERMS_NOTE.to_owned()),
        }
    }
}

impl Default for DojEpsteinAdapter {
    fn default() -> Self {
        Self::new(DOJ_EPSTEIN_BASE_URL)
    }
}

impl SourceAdapter for DojEpsteinAdapter {
    fn name(&self) -> &'static str {
        DOJ_EPSTEIN_SOURCE
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
                    DOJ_EPSTEIN_SOURCE,
                    "DOJ Epstein search expects a non-empty query string.",
                    Some(
                        "Try terms such as 'data set 1', 'court records', 'foia', 'bop video', or a case name."
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
                DojEpsteinLocator::Url(url) => self.get_record_by_url(&url).await,
                DojEpsteinLocator::SourceId(source_id) => {
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

pub fn doj_epstein_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn doj_epstein_terms_note() -> &'static str {
    TERMS_NOTE
}
