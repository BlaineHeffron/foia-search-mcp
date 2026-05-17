use reqwest::header::{CONTENT_TYPE, LOCATION, USER_AGENT};
use reqwest::redirect::Policy;
use serde_json::Value;
use std::time::Duration;

use crate::sources::SourceError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT_VALUE: &str = "foia-search-mcp/0.1 (+https://github.com/modelcontextprotocol)";
const SOURCE_CHANGED_WARNING: &str =
    "GovInfo returned a non-JSON response. Verify GovInfo API availability and the requested identifier.";

pub(crate) async fn fetch_json_get(source: &'static str, url: &str) -> Result<Value, SourceError> {
    let text = crate::http::fetch_text(source, url).await?;
    parse_json_text(source, &text, url)
}

pub(crate) async fn fetch_json_post(
    source: &'static str,
    url: &str,
    body: &Value,
) -> Result<Value, SourceError> {
    let response = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(Policy::none())
        .build()
        .map_err(|err| SourceError::Fetch {
            source,
            message: format!("Failed to initialize HTTP client: {err}"),
            url: Some(url.to_owned()),
        })?
        .post(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .body(body.to_string())
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

    let text = response.text().await.map_err(|err| SourceError::Fetch {
        source,
        message: format!("Failed to read HTTP response body: {err}"),
        url: Some(url.to_owned()),
    })?;

    parse_json_text(source, &text, url)
}

pub(crate) fn percent_encode_query(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(crate) fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn parse_json_text(source: &'static str, text: &str, url: &str) -> Result<Value, SourceError> {
    if text.trim_start().starts_with('<') {
        return Err(SourceError::SourceChanged {
            source,
            message: SOURCE_CHANGED_WARNING.to_owned(),
            url: Some(url.to_owned()),
        });
    }

    serde_json::from_str(text).map_err(|err| SourceError::SourceChanged {
        source,
        message: format!(
            "GovInfo returned invalid JSON. Verify API key, endpoint, and source availability. Details: {err}"
        ),
        url: Some(url.to_owned()),
    })
}
