use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture,
    SourceMetadata, SourceRecord, SourceStatus,
};

mod assets;
mod parse;
mod url;

use assets::{build_assets, dedupe_and_sort_assets};
use parse::{record_from_detail_html, records_from_search_html};
use url::{catalog_endpoint, detail_endpoint, document_key, parse_locator, FrusLocator};

pub const FRUS_SOURCE: &str = "frus";
pub const FRUS_SEARCH_SOURCE: &str = "history_state_gov_frus";
pub const FRUS_API_ROOT: &str = "https://history.state.gov";

const CITATION_NOTE: &str = "FRUS official Office of the Historian record. Cite the canonical history.state.gov document URL and document number when present; verify date and volume context before publication.";
const TERMS_NOTE: &str = "FRUS content is public domain U.S. government material. Use canonical Office of the Historian URLs, preserve document-level provenance, and avoid non-official mirrors for citation.";
pub(crate) const SOURCE_CHANGED_WARNING: &str =
    "FRUS catalog/detail format may have changed. Verify the Office of the Historian FRUS record manually.";

#[derive(Debug, Clone)]
pub struct FrusAdapter {
    api_root: String,
}

impl FrusAdapter {
    pub fn new(api_root: impl Into<String>) -> Self {
        Self {
            api_root: api_root.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_FRUS_BASE_URL")
            .or_else(|_| std::env::var("FOIA_SEARCH_FRUS_API_BASE_URL"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn api_root(&self) -> &str {
        self.api_root.trim()
    }

    async fn search_catalog(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<SearchPage, SourceError> {
        let endpoint = catalog_endpoint(
            self.api_root(),
            query,
            options.max_results,
            options.cursor.as_deref(),
        );
        let body = fetch_text(FRUS_SOURCE, &endpoint).await?;
        let mut parsed = records_from_search_html(&body, &endpoint)?;
        parsed.truncate(options.max_results.min(50));

        let warnings = if parsed.is_empty() {
            vec![
                "FRUS returned no matching records. Try broader terms, a different volume, or verify the official Office of the Historian search page."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        let records = parsed
            .into_iter()
            .map(|record| source_record_from_parsed(&record, &endpoint))
            .collect();

        Ok(SearchPage {
            query: query.to_owned(),
            source: FRUS_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let normalized = source_id.trim().trim_start_matches("frus:").trim();
        if normalized.is_empty() || !normalized.contains('/') {
            return Err(SourceError::invalid_input(
                FRUS_SOURCE,
                "FRUS source_id must include both volume and document ids.",
                Some(
                    "Use <volume-id>/<document-id>, for example frus1969-76v12/d34, or use an official history.state.gov URL."
                        .to_owned(),
                ),
            ));
        }

        let endpoint = detail_endpoint(self.api_root(), normalized);
        let body = fetch_text(FRUS_SOURCE, &endpoint).await?;
        let parsed = record_from_detail_html(&body, &endpoint, Some(normalized))?;
        Ok(source_record_from_parsed(&parsed, &endpoint))
    }
}

impl Default for FrusAdapter {
    fn default() -> Self {
        Self::new(FRUS_API_ROOT)
    }
}

impl SourceAdapter for FrusAdapter {
    fn name(&self) -> &'static str {
        FRUS_SOURCE
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
                    FRUS_SOURCE,
                    "FRUS search expects a non-empty query string.",
                    Some(
                        "Try document numbers, officials, places, topics, or a specific FRUS volume id."
                            .to_owned(),
                    ),
                ));
            }
            self.search_catalog(query, &options).await
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            match parse_locator(id_or_url)? {
                FrusLocator::SourceId(source_id) => self.get_record_by_source_id(&source_id).await,
                FrusLocator::OfficialUrl(source_id) => {
                    self.get_record_by_source_id(&source_id).await
                }
            }
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(dedupe_and_sort_assets(record.attachments.clone())) })
    }
}

pub fn frus_terms_note() -> &'static str {
    TERMS_NOTE
}

pub fn frus_citation_note() -> &'static str {
    CITATION_NOTE
}

fn source_record_from_parsed(parsed: &parse::ParsedFrusRecord, origin_url: &str) -> SourceRecord {
    let source_id = parsed
        .source_id
        .trim()
        .trim_start_matches("frus:")
        .to_owned();
    let official_url = parsed.official_url.trim();

    let attachments = build_assets(
        official_url,
        parsed.tei_url.as_deref(),
        parsed.pdf_url.as_deref(),
        parsed.ebook_url.as_deref(),
    );

    let mut metadata = SourceMetadata::new();
    if let Some(volume_id) = parsed.volume_id.as_deref() {
        metadata.insert("volume_id".to_owned(), volume_id.to_owned());
    }
    if let Some(element_id) = parsed.element_id.as_deref() {
        metadata.insert("element_id".to_owned(), element_id.to_owned());
    }
    if let Some(volume_title) = parsed.volume_title.as_deref() {
        metadata.insert("volume_title".to_owned(), volume_title.to_owned());
    }
    if let Some(document_number) = parsed.document_number.as_deref() {
        metadata.insert("document_number".to_owned(), document_number.to_owned());
    }
    if let Some(date) = parsed.date.as_deref() {
        metadata.insert("document_date".to_owned(), date.to_owned());
    }
    if let Some(url) = parsed.official_volume_url.as_deref() {
        metadata.insert("official_volume_url".to_owned(), url.to_owned());
    }
    if let Some(url) = parsed.tei_url.as_deref() {
        metadata.insert("tei_xml_url".to_owned(), url.to_owned());
    }
    if let Some(url) = parsed.pdf_url.as_deref() {
        metadata.insert("pdf_url".to_owned(), url.to_owned());
    }
    if let Some(url) = parsed.ebook_url.as_deref() {
        metadata.insert("ebook_epub_url".to_owned(), url.to_owned());
    }
    if let Some(summary) = parsed.summary.as_deref() {
        metadata.insert("summary".to_owned(), summary.to_owned());
    }
    metadata.insert("official_document_url".to_owned(), official_url.to_owned());
    metadata.insert("persons".to_owned(), parsed.persons.join(" | "));
    metadata.insert("places".to_owned(), parsed.places.join(" | "));
    metadata.insert("topics".to_owned(), parsed.topics.join(" | "));
    metadata.insert("asset_count".to_owned(), attachments.len().to_string());
    metadata.insert(
        "pdf_asset_count".to_owned(),
        attachments
            .iter()
            .filter(|asset| asset.role == crate::sources::SourceAssetRole::Pdf)
            .count()
            .to_string(),
    );
    metadata.insert(
        "source_warning".to_owned(),
        "FRUS records should be cited from official history.state.gov document URLs with document-number context when available."
            .to_owned(),
    );

    SourceRecord {
        id: format!("{FRUS_SOURCE}:{source_id}"),
        document_key: document_key(&source_id),
        source: FRUS_SOURCE,
        source_id,
        title: parsed.document_title.clone(),
        date: parsed.date.clone(),
        collection: Some("Foreign Relations of the United States (FRUS)".to_owned()),
        record_group: parsed.volume_id.clone(),
        description: parsed.summary.clone(),
        origin_url: origin_url.to_owned(),
        document_url: official_url.to_owned(),
        pdf_url: parsed.pdf_url.clone(),
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(CITATION_NOTE.to_owned()),
        terms_note: Some(TERMS_NOTE.to_owned()),
    }
}
