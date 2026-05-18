use crate::sources::SourceError;

use super::super::{DOE_OPENNET_BASE_URL, DOE_SOURCE};

#[derive(Debug, Clone)]
pub(crate) enum DoeLocator {
    SourceId(String),
    OfficialUrl(String),
}

pub(crate) fn parse_locator(id_or_url: &str) -> Result<DoeLocator, SourceError> {
    let mut value = id_or_url.trim();
    if value.is_empty() {
        return Err(SourceError::invalid_input(
            DOE_SOURCE,
            "DOE OpenNet lookup expects an OSTI id or official www.osti.gov OpenNet detail URL.",
            Some("Examples: doe:1824644, 1824644, https://www.osti.gov/opennet/detail?osti-id=1824644".to_owned()),
        ));
    }
    if let Some(stripped) = value.strip_prefix("doe:") {
        value = stripped.trim();
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        if !is_allowed_official_url(value) {
            return Err(SourceError::invalid_input(
                DOE_SOURCE,
                "DOE OpenNet lookup only accepts official https://www.osti.gov/opennet/detail URLs.",
                Some("Use OpenNet detail URLs such as https://www.osti.gov/opennet/detail?osti-id=<id>.".to_owned()),
            ));
        }
        let source_id = source_id_from_official_url(value).ok_or_else(|| {
            SourceError::invalid_input(
                DOE_SOURCE,
                "DOE OpenNet URL format is not recognized for record lookup.",
                Some("Expected https://www.osti.gov/opennet/detail?osti-id=<id>.".to_owned()),
            )
        })?;
        return Ok(DoeLocator::OfficialUrl(source_id));
    }

    let source_id = normalize_source_id(value).ok_or_else(|| {
        SourceError::invalid_input(
            DOE_SOURCE,
            "DOE OpenNet source_id format is not recognized.",
            Some("Use numeric OSTI ids from OpenNet detail URLs.".to_owned()),
        )
    })?;
    Ok(DoeLocator::SourceId(source_id))
}

pub(crate) fn search_endpoint(base_url: &str, start: &str) -> String {
    let page = start
        .parse::<usize>()
        .ok()
        .map(|offset| (offset / 50).saturating_add(1))
        .unwrap_or(1);
    format!(
        "{}/opennet/search-results?page={page}",
        base_url.trim_end_matches('/')
    )
}

pub(crate) fn detail_endpoint(base_url: &str, source_id: &str) -> String {
    format!(
        "{}/opennet/detail?osti-id={}",
        base_url.trim_end_matches('/'),
        percent_encode_component(source_id)
    )
}

pub(crate) fn official_record_url(source_id: &str) -> String {
    detail_endpoint(DOE_OPENNET_BASE_URL, source_id)
}

pub(crate) fn source_id_from_official_url(url: &str) -> Option<String> {
    if !is_allowed_official_url(url) {
        return None;
    }
    source_id_from_query(url)
}

pub(crate) fn source_id_from_official_url_with_base(url: &str, base_url: &str) -> Option<String> {
    if source_id_from_official_url(url).is_some() {
        return source_id_from_official_url(url);
    }
    let base = base_url.trim_end_matches('/').to_ascii_lowercase();
    let lower = url.to_ascii_lowercase();
    if lower.starts_with(&format!("{base}/opennet/detail?")) && lower.contains("osti-id=") {
        return source_id_from_query(url);
    }
    None
}

pub(crate) fn document_key(source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in DOE_SOURCE
        .bytes()
        .chain([b':'])
        .chain(source_id.trim().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{DOE_SOURCE}-{hash:016x}")
}

pub(crate) fn absolutize(base_url: &str, href: &str) -> String {
    if href.starts_with("https://") {
        return href.to_owned();
    }
    if href.starts_with("http://www.osti.gov/opennet/") {
        return format!("https://{}", &href["http://".len()..]);
    }
    let base = base_url.trim_end_matches('/');
    if href.starts_with('/') {
        return format!("{base}{href}");
    }
    format!("{base}/{href}")
}

fn is_allowed_official_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://www.osti.gov/opennet/detail?") && lower.contains("osti-id=")
}

fn source_id_from_query(url: &str) -> Option<String> {
    url.split(['?', '&'])
        .find_map(|part| part.strip_prefix("osti-id="))
        .and_then(normalize_source_id)
}

pub(crate) fn normalize_source_id(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('/');
    if !trimmed.is_empty() && trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
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
    fn official_url_accepts_opennet_detail_https_only() {
        assert!(is_allowed_official_url(
            "https://www.osti.gov/opennet/detail?osti-id=1824644"
        ));
        assert!(!is_allowed_official_url(
            "http://www.osti.gov/opennet/detail?osti-id=1824644"
        ));
        assert!(!is_allowed_official_url(
            "https://example.com/opennet/detail?osti-id=1824644"
        ));
    }
}
