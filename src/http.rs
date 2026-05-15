use std::time::Duration;

use reqwest::header::USER_AGENT;

use crate::sources::SourceError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT_VALUE: &str = "foia-search-mcp/0.1 (+https://github.com/modelcontextprotocol)";

pub async fn fetch_text(source: &'static str, url: &str) -> Result<String, SourceError> {
    let response = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|err| SourceError::Fetch {
            source,
            message: format!("Failed to initialize HTTP client: {err}"),
            url: Some(url.to_owned()),
        })?
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
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
