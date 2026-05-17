use std::time::Duration;

use reqwest::header::{HeaderMap, LOCATION, USER_AGENT};
use reqwest::redirect::Policy;

use crate::sources::SourceError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT_VALUE: &str = "foia-search-mcp/0.1 (+https://github.com/modelcontextprotocol)";

pub async fn fetch_text(source: &'static str, url: &str) -> Result<String, SourceError> {
    fetch_text_with_headers(source, url, HeaderMap::new()).await
}

pub async fn fetch_text_with_headers(
    source: &'static str,
    url: &str,
    headers: HeaderMap,
) -> Result<String, SourceError> {
    let response = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(Policy::none())
        .build()
        .map_err(|err| SourceError::Fetch {
            source,
            message: format!("Failed to initialize HTTP client: {err}"),
            url: Some(url.to_owned()),
        })?
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .headers(headers)
        .send()
        .await
        .map_err(|err| SourceError::Fetch {
            source,
            message: format!(
                "HTTP request failed. Retry later, narrow the query, or verify the source page manually. Details: {err}"
            ),
            url: Some(url.to_owned()),
        })?;

    let status = response.status();
    if status.is_redirection() {
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let location_note = location
            .as_deref()
            .map(|value| format!(" Redirect location: {value}"))
            .unwrap_or_default();
        return Err(SourceError::Fetch {
            source,
            message: format!(
                "Source returned redirect HTTP {status}. Redirect responses are denied by default for source text fetches.{location_note}"
            ),
            url: Some(url.to_owned()),
        });
    }
    if !status.is_success() {
        return Err(SourceError::Fetch {
            source,
            message: format!(
                "Source returned HTTP {status}. Retry later, narrow the query, or verify the source page manually."
            ),
            url: Some(url.to_owned()),
        });
    }

    response.text().await.map_err(|err| SourceError::Fetch {
        source,
        message: format!("Failed to read HTTP response body: {err}"),
        url: Some(url.to_owned()),
    })
}
