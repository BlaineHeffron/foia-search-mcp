use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod assets;
mod html;
mod parse;
mod url;

use assets::dedupe_and_sort_assets;
use parse::record_from_detail_html;
use url::{accessions_from_query, citation_endpoint, parse_locator, DticLocator};

pub const DTIC_SOURCE: &str = "dtic";
pub const DTIC_SEARCH_SOURCE: &str = "dtic_public_tracer";
pub const DTIC_BASE_URL: &str = "https://apps.dtic.mil";
pub const DTIC_DISCOVER_SEARCH_URL: &str = "https://discover.dtic.mil/results/?q={query}";

const CITATION_NOTE: &str = "DTIC record metadata should be cited from the official DTIC citation page and the official DTIC PDF URL when available.";
const TERMS_NOTE: &str = "DTIC public access is fragile and coverage varies. Use official DTIC URLs, preserve distribution/public-release statements, and treat non-official mirrors as out-of-scope for citation.";
const SEARCH_LIMITATION_WARNING: &str =
    "DTIC public search endpoints are not treated as stable APIs. This adapter only performs accession-driven official-record lookups and returns guarded warnings for broad text-only queries.";

#[derive(Debug, Clone)]
pub struct DticAdapter {
    base_url: String,
}

impl DticAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_DTIC_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> &str {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            DTIC_BASE_URL
        } else {
            trimmed
        }
    }

    async fn get_record_by_accession(&self, accession: &str) -> Result<SourceRecord, SourceError> {
        let endpoint = citation_endpoint(self.base_url(), accession);
        let body = fetch_text(DTIC_SOURCE, &endpoint).await?;
        record_from_detail_html(&body, self.base_url(), &endpoint, Some(accession))
    }
}

impl Default for DticAdapter {
    fn default() -> Self {
        Self::new(DTIC_BASE_URL)
    }
}

impl SourceAdapter for DticAdapter {
    fn name(&self) -> &'static str {
        DTIC_SOURCE
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
                    DTIC_SOURCE,
                    "DTIC search expects a non-empty query string.",
                    Some(
                        "Provide a DTIC accession id such as ADA630142 or use get_source_record with an official DTIC URL."
                            .to_owned(),
                    ),
                ));
            }

            let mut warnings = vec![SEARCH_LIMITATION_WARNING.to_owned()];
            let accessions = accessions_from_query(query);
            if accessions.is_empty() {
                warnings.push(
                    "No DTIC accession id was detected in this query. Try AD*/ADA* identifiers or use discover.dtic.mil manually and then call get_source_record with an official DTIC citation URL."
                        .to_owned(),
                );
                return Ok(SearchPage {
                    query: query.to_owned(),
                    source: DTIC_SEARCH_SOURCE,
                    records: Vec::new(),
                    next_cursor: None,
                    warnings,
                });
            }

            let mut records = Vec::new();
            for accession in accessions
                .into_iter()
                .take(options.max_results.clamp(1, 25))
            {
                match self.get_record_by_accession(&accession).await {
                    Ok(record) => records.push(record),
                    Err(err) => warnings.push(format!(
                        "DTIC accession {accession} could not be resolved automatically: {err}"
                    )),
                }
            }

            if records.is_empty() {
                warnings.push(
                    "No DTIC records were resolved from accession ids in this query. Verify the accession values on official DTIC pages before retrying."
                        .to_owned(),
                );
            }

            Ok(SearchPage {
                query: query.to_owned(),
                source: DTIC_SEARCH_SOURCE,
                records,
                next_cursor: None,
                warnings,
            })
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            match parse_locator(id_or_url)? {
                DticLocator::Accession(accession)
                | DticLocator::OfficialCitationUrl(accession)
                | DticLocator::OfficialPdfUrl(accession) => {
                    self.get_record_by_accession(&accession).await
                }
            }
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(dedupe_and_sort_assets(record.attachments.clone())) })
    }
}

pub fn dtic_terms_note() -> &'static str {
    TERMS_NOTE
}

pub fn dtic_citation_note() -> &'static str {
    CITATION_NOTE
}
