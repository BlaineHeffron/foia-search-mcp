use crate::sources::SourceError;

use super::FRUS_SOURCE;

pub(crate) const FRUS_OFFICIAL_HOST: &str = "history.state.gov";

#[derive(Debug, Clone)]
pub(crate) enum FrusLocator {
    SourceId(String),
    OfficialUrl(String),
}

pub(crate) fn parse_locator(id_or_url: &str) -> Result<FrusLocator, SourceError> {
    let mut value = id_or_url.trim();
    if value.is_empty() {
        return Err(SourceError::invalid_input(
            FRUS_SOURCE,
            "FRUS lookup expects a source id or official history.state.gov URL.",
            Some(
                "Examples: frus:frus1969-76v12/d34, frus1969-76v12/d34, or https://history.state.gov/historicaldocuments/frus1969-76v12/d34"
                    .to_owned(),
            ),
        ));
    }

    if let Some(stripped) = value.strip_prefix("frus:") {
        value = stripped.trim();
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        if !is_allowed_official_url(value) {
            return Err(SourceError::invalid_input(
                FRUS_SOURCE,
                "FRUS lookup only accepts official history.state.gov URLs.",
                Some(
                    "Use canonical URLs such as https://history.state.gov/historicaldocuments/<volume-id>/<document-id>."
                        .to_owned(),
                ),
            ));
        }
        let source_id = source_id_from_official_url(value).ok_or_else(|| {
            SourceError::invalid_input(
                FRUS_SOURCE,
                "FRUS URL format is not recognized for record lookup.",
                Some(
                    "Expected https://history.state.gov/historicaldocuments/<volume-id>/<document-id>"
                        .to_owned(),
                ),
            )
        })?;
        return Ok(FrusLocator::OfficialUrl(source_id));
    }

    Ok(FrusLocator::SourceId(value.to_owned()))
}

pub(crate) fn source_id_from_official_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let marker = "/historicaldocuments/";
    let marker_index = lower.find(marker)? + marker.len();
    let tail = &url[marker_index..];
    let cleaned = tail
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_matches('/');
    let mut parts = cleaned.split('/');
    let volume_id = parts.next()?.trim();
    let element_id = parts.next()?.trim();
    if volume_id.is_empty() || element_id.is_empty() {
        return None;
    }
    Some(format!("{volume_id}/{element_id}"))
}

pub(crate) fn source_id_from_historicaldocuments_path(path_or_url: &str) -> Option<String> {
    let value = path_or_url.trim();
    if value.starts_with("https://") {
        return source_id_from_official_url(value);
    }

    let lower = value.to_ascii_lowercase();
    let marker = "/historicaldocuments/";
    let marker_index = lower.find(marker)? + marker.len();
    let tail = &value[marker_index..];
    let cleaned = tail
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_matches('/');
    let mut parts = cleaned.split('/');
    let volume_id = parts.next()?.trim();
    let element_id = parts.next()?.trim();
    if volume_id.is_empty() || element_id.is_empty() {
        return None;
    }
    Some(format!("{volume_id}/{element_id}"))
}

pub(crate) fn is_document_source_id(source_id: &str) -> bool {
    source_id
        .split('/')
        .nth(1)
        .map(|element| {
            let mut chars = element.chars();
            matches!(chars.next(), Some('d')) && chars.all(|ch| ch.is_ascii_digit())
        })
        .unwrap_or(false)
}

pub(crate) fn is_allowed_official_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return false;
    }
    if !(lower.starts_with("https://history.state.gov/")
        || lower.starts_with("https://www.history.state.gov/"))
    {
        return false;
    }

    lower.contains("/historicaldocuments/")
}

pub(crate) fn catalog_endpoint(
    api_root: &str,
    query: &str,
    limit: usize,
    cursor: Option<&str>,
) -> String {
    let mut url = format!(
        "{}/search?within=documents&q={}",
        api_root.trim_end_matches('/'),
        percent_encode_component(query)
    );
    let _ = limit;
    if let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) {
        url.push_str("&start=");
        url.push_str(&percent_encode_component(cursor));
    }
    url
}

pub(crate) fn detail_endpoint(api_root: &str, source_id: &str) -> String {
    let normalized = source_id.trim().trim_start_matches("frus:").trim();
    let mut parts = normalized.split('/');
    let volume_id = parts.next().unwrap_or_default();
    let element_id = parts.next().unwrap_or_default();
    format!(
        "{}/historicaldocuments/{}/{}",
        api_root.trim_end_matches('/'),
        percent_encode_component(volume_id),
        percent_encode_component(element_id)
    )
}

pub(crate) fn official_document_url_for_source_id(source_id: &str) -> String {
    let normalized = source_id.trim().trim_start_matches("frus:").trim();
    let segments: Vec<&str> = normalized.split('/').collect();
    if segments.len() >= 2 {
        format!(
            "https://{FRUS_OFFICIAL_HOST}/historicaldocuments/{}/{}",
            segments[0], segments[1]
        )
    } else {
        format!(
            "https://{FRUS_OFFICIAL_HOST}/historicaldocuments/{}",
            normalized.trim_matches('/')
        )
    }
}

pub(crate) fn document_key(source_id: &str) -> String {
    let mut normalized = String::with_capacity(source_id.len());
    for ch in source_id.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        } else {
            normalized.push('-');
        }
    }
    while normalized.contains("--") {
        normalized = normalized.replace("--", "-");
    }
    let normalized = normalized.trim_matches('-');
    format!("{FRUS_SOURCE}-{normalized}")
}

fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        let is_unreserved =
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_source_id_from_official_url() {
        let source_id = source_id_from_official_url(
            "https://history.state.gov/historicaldocuments/frus1969-76v12/d34",
        )
        .expect("url should parse");
        assert_eq!(source_id, "frus1969-76v12/d34");
    }

    #[test]
    fn rejects_non_official_urls() {
        assert!(!is_allowed_official_url(
            "https://example.com/historicaldocuments/frus1969-76v12/d34"
        ));
        assert!(!is_allowed_official_url(
            "http://history.state.gov/historicaldocuments/frus1969-76v12/d34"
        ));
    }
}
