use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod live;

use live::{
    detail_endpoint, parse_locator, record_from_detail_html, records_from_search_html,
    search_endpoint, DoeLocator,
};

pub const DOE_SOURCE: &str = "doe";
pub const DOE_SEARCH_SOURCE: &str = "doe_opennet_search";
pub const DOE_OPENNET_BASE_URL: &str = "https://www.osti.gov";

const CITATION_NOTE: &str = "DOE OpenNet metadata is an official lead finder. Cite the official OpenNet detail page and any linked PDF; page citations require PDF ingestion and page-boundary verification.";
const TERMS_NOTE: &str = "Use official DOE/OSTI OpenNet pages and assets. Respect source cache headers/rate limits, preserve OpenNet accession/location metadata, and treat non-PDF assets as conservative metadata leads.";

#[derive(Debug, Clone)]
pub struct DoeAdapter {
    base_url: String,
}

impl DoeAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_DOE_OPENNET_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> &str {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            DOE_OPENNET_BASE_URL
        } else {
            trimmed
        }
    }

    async fn search_opennet(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<SearchPage, SourceError> {
        let start = options.cursor.as_deref().unwrap_or("0");
        let endpoint = search_endpoint(self.base_url(), start);
        let body = post_search_form(self.base_url(), &endpoint, query, start).await?;
        let mut records = records_from_search_html(&body, self.base_url(), &endpoint)?;
        records.truncate(options.max_results.min(50));

        let next_cursor = if records.len() >= options.max_results.min(50) {
            start.parse::<usize>().ok().map(|value| {
                let next = value.saturating_add(options.max_results.min(50));
                next.to_string()
            })
        } else {
            None
        };

        let warnings = if records.is_empty() {
            vec![
                "DOE OpenNet returned no matching records. Try broader terms, accession numbers, document numbers, field-office acronyms, or title keywords."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: DOE_SEARCH_SOURCE,
            records,
            next_cursor,
            warnings,
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let endpoint = detail_endpoint(self.base_url(), source_id);
        let body = fetch_text(DOE_SOURCE, &endpoint).await?;
        record_from_detail_html(&body, self.base_url(), &endpoint, Some(source_id))
    }
}

impl Default for DoeAdapter {
    fn default() -> Self {
        Self::new(DOE_OPENNET_BASE_URL)
    }
}

impl SourceAdapter for DoeAdapter {
    fn name(&self) -> &'static str {
        DOE_SOURCE
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
                    DOE_SOURCE,
                    "DOE OpenNet search expects a non-empty query string.",
                    Some(
                        "Try accession numbers, document numbers, title keywords, field offices, or declassified-record topics."
                            .to_owned(),
                    ),
                ));
            }

            self.search_opennet(query, &options).await
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            match parse_locator(id_or_url)? {
                DoeLocator::SourceId(source_id) | DoeLocator::OfficialUrl(source_id) => {
                    self.get_record_by_source_id(&source_id).await
                }
            }
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move {
            let mut assets = record.attachments.clone();
            assets.sort_by(|left, right| {
                asset_rank(left)
                    .cmp(&asset_rank(right))
                    .then_with(|| left.asset_url.cmp(&right.asset_url))
            });
            assets.dedup_by(|left, right| left.asset_url == right.asset_url);
            Ok(assets)
        })
    }
}

pub fn doe_terms_note() -> &'static str {
    TERMS_NOTE
}

pub fn doe_citation_note() -> &'static str {
    CITATION_NOTE
}

async fn post_search_form(
    base_url: &str,
    endpoint: &str,
    query: &str,
    start: &str,
) -> Result<String, SourceError> {
    use reqwest::header::USER_AGENT;
    use reqwest::redirect::Policy;
    use std::time::Duration;

    let length = "50";
    let form = [
        ("search-for", query),
        ("full-text", ""),
        ("document-categories", "[]"),
        ("declassification-status", "[]"),
        ("accession-number", ""),
        ("document-number", ""),
        ("title", ""),
        ("author", ""),
        ("addressee", ""),
        ("document-location", "[]"),
        ("opennet-field-office-acronym", ""),
        ("description", ""),
        ("document-type", "[]"),
        ("originating-research-organization", ""),
        ("publication-start-date", ""),
        ("publication-end-date", ""),
        ("declassification-start-date", ""),
        ("declassification-end-date", ""),
        ("database-entry-start-date", ""),
        ("database-entry-end-date", ""),
        ("modified-start-date", ""),
        ("modified-end-date", ""),
        ("sort-by", "RELV"),
        ("order-by", "desc"),
        ("search-form-page-num", "1"),
        ("start", start),
        ("length", length),
        ("search-extra", ""),
        ("search-extra-field", ""),
    ];

    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(Policy::none())
        .build()
        .map_err(|err| SourceError::Fetch {
            source: DOE_SOURCE,
            message: format!("Failed to initialize HTTP client: {err}"),
            url: Some(endpoint.to_owned()),
        })?
        .post(endpoint)
        .header(
            USER_AGENT,
            "foia-search-mcp/0.1 (+https://github.com/modelcontextprotocol)",
        )
        .header(
            "Referer",
            format!("{}/opennet/", base_url.trim_end_matches('/')),
        )
        .form(&form)
        .send()
        .await
        .map_err(|err| SourceError::Fetch {
            source: DOE_SOURCE,
            message: format!("DOE OpenNet search request failed: {err}"),
            url: Some(endpoint.to_owned()),
        })?;

    let status = response.status();
    if status.is_redirection() {
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        return Err(SourceError::Fetch {
            source: DOE_SOURCE,
            message: format!(
                "DOE OpenNet returned redirect HTTP {status}. Redirect responses are denied by default for source text fetches. Redirect location: {location}"
            ),
            url: Some(endpoint.to_owned()),
        });
    }
    if !status.is_success() {
        return Err(SourceError::Fetch {
            source: DOE_SOURCE,
            message: format!("DOE OpenNet returned HTTP {status}. Retry later or verify the official OpenNet search page manually."),
            url: Some(endpoint.to_owned()),
        });
    }

    response.text().await.map_err(|err| SourceError::Fetch {
        source: DOE_SOURCE,
        message: format!("Failed to read DOE OpenNet response body: {err}"),
        url: Some(endpoint.to_owned()),
    })
}

fn asset_rank(asset: &SourceAsset) -> u8 {
    match asset.role {
        crate::sources::SourceAssetRole::Pdf => 0,
        crate::sources::SourceAssetRole::Html => 1,
        _ => 2,
    }
}
