use crate::sources::SourceError;

use super::NOAA_SOURCE;

const NOAA_OFFICIAL_HOST: &str = "repository.library.noaa.gov";

#[derive(Debug, Clone)]
pub(crate) enum NoaaLocator {
    SourceId(String),
    OfficialUrl(String),
}

pub(crate) fn parse_locator(id_or_url: &str) -> Result<NoaaLocator, SourceError> {
    let mut value = id_or_url.trim();
    if value.is_empty() {
        return Err(SourceError::invalid_input(
            NOAA_SOURCE,
            "NOAA lookup expects a source id or official repository.library.noaa.gov URL.",
            Some(
                "Examples: noaa:16063, 16063, https://repository.library.noaa.gov/view/noaa/16063"
                    .to_owned(),
            ),
        ));
    }

    if let Some(stripped) = value.strip_prefix("noaa:") {
        value = stripped.trim();
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        if !is_allowed_official_url(value) {
            return Err(SourceError::invalid_input(
                NOAA_SOURCE,
                "NOAA lookup only accepts official repository.library.noaa.gov URLs.",
                Some(
                    "Use canonical NOAA repository URLs such as https://repository.library.noaa.gov/view/noaa/<id>."
                        .to_owned(),
                ),
            ));
        }
        let source_id = source_id_from_official_url(value).ok_or_else(|| {
            SourceError::invalid_input(
                NOAA_SOURCE,
                "NOAA URL format is not recognized for record lookup.",
                Some(
                    "Expected https://repository.library.noaa.gov/view/noaa/<id> or /handle/noaa.<id>."
                        .to_owned(),
                ),
            )
        })?;
        return Ok(NoaaLocator::OfficialUrl(source_id));
    }

    let source_id = normalize_source_id(value).ok_or_else(|| {
        SourceError::invalid_input(
            NOAA_SOURCE,
            "NOAA source_id format is not recognized.",
            Some(
                "Use numeric repository item ids such as 16063, or official repository URLs from search results."
                    .to_owned(),
            ),
        )
    })?;

    Ok(NoaaLocator::SourceId(source_id))
}

pub(crate) fn normalize_source_id(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains("://") {
        return source_id_from_official_url(trimmed);
    }

    if let Some(id) = source_id_from_official_path(trimmed) {
        return Some(id);
    }

    if let Some(handle) = trimmed.strip_prefix("handle/") {
        return source_id_from_handle_value(handle);
    }

    if let Some(handle) = trimmed.strip_prefix("noaa.") {
        let id = handle.trim();
        if id.chars().all(|ch| ch.is_ascii_digit()) {
            return Some(id.to_owned());
        }
    }

    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Some(trimmed.to_owned());
    }

    None
}

pub(crate) fn source_id_from_official_url(url: &str) -> Option<String> {
    if !is_allowed_official_url(url) {
        return None;
    }
    let (_, rest) = url.split_once("://")?;
    let tail = rest.split_once('/').map(|(_, path)| path).unwrap_or("");
    source_id_from_official_path(tail)
}

fn source_id_from_official_path(path: &str) -> Option<String> {
    let cleaned = path
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_matches('/');

    let segments = cleaned
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();

    if segments.len() >= 3
        && segments[0].eq_ignore_ascii_case("view")
        && segments[1].eq_ignore_ascii_case("noaa")
    {
        let id = segments[2].trim();
        if !id.is_empty() {
            return Some(id.to_owned());
        }
    }

    if segments.len() >= 2 && segments[0].eq_ignore_ascii_case("handle") {
        return source_id_from_handle_value(segments[1]);
    }

    None
}

fn source_id_from_handle_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if let Some(id) = trimmed.strip_prefix("noaa.") {
        let id = id.trim();
        if id.chars().all(|ch| ch.is_ascii_digit()) {
            return Some(id.to_owned());
        }
    }
    None
}

pub(crate) fn is_allowed_official_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return false;
    }

    if !(lower.starts_with("https://repository.library.noaa.gov/")
        || lower.starts_with("https://www.repository.library.noaa.gov/"))
    {
        return false;
    }

    lower.contains("/view/noaa/") || lower.contains("/handle/noaa.")
}

pub(crate) fn detail_endpoint(base_url: &str, source_id: &str) -> String {
    format!(
        "{}/view/noaa/{}",
        base_url.trim_end_matches('/'),
        percent_encode_component(source_id.trim())
    )
}

pub(crate) fn search_endpoint(base_url: &str, query: &str, cursor: Option<&str>) -> String {
    let mut endpoint = format!(
        "{}/search?query={}",
        base_url.trim_end_matches('/'),
        percent_encode_component(query)
    );

    if let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) {
        endpoint.push_str("&start=");
        endpoint.push_str(&percent_encode_component(cursor));
    }

    endpoint
}

pub(crate) fn official_record_url(source_id: &str) -> String {
    format!(
        "https://{NOAA_OFFICIAL_HOST}/view/noaa/{}",
        percent_encode_component(source_id.trim())
    )
}

pub(crate) fn document_key(source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in NOAA_SOURCE
        .bytes()
        .chain([b':'])
        .chain(source_id.trim().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{NOAA_SOURCE}-{hash:016x}")
}

pub(crate) fn absolutize(base_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_owned();
    }

    let base = base_url.trim_end_matches('/');
    if href.starts_with('/') {
        return format!("{base}{href}");
    }

    format!("{base}/{href}")
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
    fn parses_source_id_from_official_view_url() {
        let id = source_id_from_official_url("https://repository.library.noaa.gov/view/noaa/16063")
            .expect("id should parse");
        assert_eq!(id, "16063");
    }

    #[test]
    fn rejects_non_official_or_http_urls() {
        assert!(!is_allowed_official_url(
            "https://example.com/view/noaa/16063"
        ));
        assert!(!is_allowed_official_url(
            "http://repository.library.noaa.gov/view/noaa/16063"
        ));
    }
}
