use crate::sources::SourceError;

use super::DTIC_SOURCE;

const DTIC_OFFICIAL_HOST: &str = "apps.dtic.mil";

#[derive(Debug, Clone)]
pub(crate) enum DticLocator {
    Accession(String),
    OfficialCitationUrl(String),
    OfficialPdfUrl(String),
}

pub(crate) fn parse_locator(id_or_url: &str) -> Result<DticLocator, SourceError> {
    let mut value = id_or_url.trim();
    if value.is_empty() {
        return Err(SourceError::invalid_input(
            DTIC_SOURCE,
            "DTIC lookup expects an accession id or official DTIC citation/PDF URL.",
            Some(
                "Examples: dtic:ADA630142, ADA630142, https://apps.dtic.mil/sti/citations/ADA630142"
                    .to_owned(),
            ),
        ));
    }

    if let Some(stripped) = value.strip_prefix("dtic:") {
        value = stripped.trim();
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        if !is_allowed_official_url(value) {
            return Err(SourceError::invalid_input(
                DTIC_SOURCE,
                "DTIC lookup only accepts official https://apps.dtic.mil citation/PDF URLs.",
                Some(
                    "Use official URLs such as https://apps.dtic.mil/sti/citations/<accession>."
                        .to_owned(),
                ),
            ));
        }
        let accession = source_id_from_official_url(value).ok_or_else(|| {
            SourceError::invalid_input(
                DTIC_SOURCE,
                "DTIC URL format is not recognized for record lookup.",
                Some(
                    "Expected /sti/citations/<accession>, /sti/pdfs/<accession>.pdf, or /sti/tr/pdf/<accession>.pdf."
                        .to_owned(),
                ),
            )
        })?;

        if value.to_ascii_lowercase().contains("/sti/citations/") {
            return Ok(DticLocator::OfficialCitationUrl(accession));
        }
        return Ok(DticLocator::OfficialPdfUrl(accession));
    }

    let accession = normalize_accession(value).ok_or_else(|| {
        SourceError::invalid_input(
            DTIC_SOURCE,
            "DTIC accession format is not recognized.",
            Some(
                "Use AD*/ADA* style accession ids (for example ADA630142) or an official DTIC citation URL."
                    .to_owned(),
            ),
        )
    })?;

    Ok(DticLocator::Accession(accession))
}

pub(crate) fn accessions_from_query(query: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for token in query.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        if let Some(accession) = normalize_accession(token) {
            if seen.insert(accession.clone()) {
                values.push(accession);
            }
        }
    }

    values
}

pub(crate) fn normalize_accession(value: &str) -> Option<String> {
    let trimmed = value
        .trim()
        .trim_matches('/')
        .trim_end_matches(".pdf")
        .to_ascii_uppercase();

    if trimmed.len() < 7 || trimmed.len() > 16 {
        return None;
    }
    if !trimmed.starts_with("AD") {
        return None;
    }
    if trimmed
        .bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Some(trimmed)
    } else {
        None
    }
}

pub(crate) fn is_allowed_official_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://apps.dtic.mil/")
        || lower.starts_with("https://www.apps.dtic.mil/"))
    {
        return false;
    }

    lower.contains("/sti/citations/")
        || lower.contains("/sti/pdfs/")
        || lower.contains("/sti/tr/pdf/")
}

pub(crate) fn source_id_from_official_url(url: &str) -> Option<String> {
    if !is_allowed_official_url(url) {
        return None;
    }
    let (_, rest) = url.split_once("://")?;
    let path = rest
        .split_once('/')
        .map(|(_, value)| value)
        .unwrap_or_default();
    source_id_from_official_path(path)
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
        && segments[0].eq_ignore_ascii_case("sti")
        && segments[1].eq_ignore_ascii_case("citations")
    {
        return normalize_accession(segments[2]);
    }

    if segments.len() >= 3
        && segments[0].eq_ignore_ascii_case("sti")
        && segments[1].eq_ignore_ascii_case("pdfs")
    {
        return normalize_accession(segments[2]);
    }

    if segments.len() >= 4
        && segments[0].eq_ignore_ascii_case("sti")
        && segments[1].eq_ignore_ascii_case("tr")
        && segments[2].eq_ignore_ascii_case("pdf")
    {
        return normalize_accession(segments[3]);
    }

    None
}

pub(crate) fn citation_endpoint(base_url: &str, accession: &str) -> String {
    format!(
        "{}/sti/citations/{}",
        base_url.trim_end_matches('/'),
        percent_encode_component(accession.trim())
    )
}

pub(crate) fn official_citation_url(accession: &str) -> String {
    format!(
        "https://{DTIC_OFFICIAL_HOST}/sti/citations/{}",
        percent_encode_component(accession.trim())
    )
}

pub(crate) fn is_official_pdf_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    is_allowed_official_url(url)
        && (lower.contains("/sti/pdfs/") || lower.contains("/sti/tr/pdf/"))
        && lower.ends_with(".pdf")
}

pub(crate) fn document_key(source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in DTIC_SOURCE
        .bytes()
        .chain([b':'])
        .chain(source_id.trim().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{DTIC_SOURCE}-{hash:016x}")
}

pub(crate) fn absolutize(base_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_owned();
    }

    if href.starts_with("/sti/") {
        return format!("https://{DTIC_OFFICIAL_HOST}{href}");
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
    fn parses_accession_from_citation_url() {
        let accession =
            source_id_from_official_url("https://apps.dtic.mil/sti/citations/ADA630142")
                .expect("accession should parse");
        assert_eq!(accession, "ADA630142");
    }

    #[test]
    fn rejects_non_official_and_http_urls() {
        assert!(!is_allowed_official_url(
            "https://example.com/sti/citations/ADA630142"
        ));
        assert!(!is_allowed_official_url(
            "http://apps.dtic.mil/sti/citations/ADA630142"
        ));
    }
}
