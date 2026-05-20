use crate::sources::{SourceAsset, SourceAssetRole};
use reqwest::Url;
use serde_json::Value;
use std::collections::HashSet;

use super::transport::percent_encode_path_segment;

const OFFICIAL_GOVINFO_API_HOST: &str = "api.govinfo.gov";

pub(crate) fn attachments_from_download(
    download: Option<&Value>,
    package_id: &str,
    granule_id: Option<&str>,
) -> Vec<SourceAsset> {
    let mut assets = Vec::new();
    let mut seen = HashSet::new();
    let Some(download_map) = download.and_then(Value::as_object) else {
        return assets;
    };

    let preferred_order = [
        "pdfLink",
        "xmlLink",
        "modsLink",
        "txtLink",
        "htmLink",
        "zipLink",
        "premisLink",
    ];

    for key in preferred_order {
        if let Some(url) = download_map.get(key).and_then(Value::as_str) {
            let url = url.trim();
            if url.is_empty() || !is_official_download_url(key, url, package_id, granule_id) {
                continue;
            }
            if seen.insert(url.to_owned()) {
                assets.push(SourceAsset {
                    asset_url: url.to_owned(),
                    label: key.trim_end_matches("Link").to_uppercase(),
                    mime_type: mime_type_for_download_key(key),
                    role: role_for_download_key(key),
                });
            }
        }
    }

    let mut remaining_keys = download_map.keys().cloned().collect::<Vec<_>>();
    remaining_keys.sort();
    for key in remaining_keys {
        if preferred_order.contains(&key.as_str()) {
            continue;
        }
        let Some(url) = download_map.get(&key).and_then(Value::as_str) else {
            continue;
        };
        let url = url.trim();
        if url.is_empty()
            || !is_official_download_url(&key, url, package_id, granule_id)
            || !seen.insert(url.to_owned())
        {
            continue;
        }
        assets.push(SourceAsset {
            asset_url: url.to_owned(),
            label: key.trim_end_matches("Link").to_uppercase(),
            mime_type: None,
            role: SourceAssetRole::Other,
        });
    }

    assets
}

fn role_for_download_key(key: &str) -> SourceAssetRole {
    match key {
        "pdfLink" => SourceAssetRole::Pdf,
        "xmlLink" | "modsLink" => SourceAssetRole::Other,
        "txtLink" => SourceAssetRole::Transcript,
        "htmLink" => SourceAssetRole::Html,
        _ => SourceAssetRole::Other,
    }
}

fn mime_type_for_download_key(key: &str) -> Option<String> {
    match key {
        "pdfLink" => Some("application/pdf".to_owned()),
        "xmlLink" => Some("application/xml".to_owned()),
        "modsLink" => Some("application/mods+xml".to_owned()),
        "txtLink" => Some("text/plain".to_owned()),
        "htmLink" => Some("text/html".to_owned()),
        _ => None,
    }
}

fn is_official_download_url(
    download_key: &str,
    url: &str,
    package_id: &str,
    granule_id: Option<&str>,
) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" || parsed.host_str() != Some(OFFICIAL_GOVINFO_API_HOST) {
        return false;
    }

    let encoded_package_id = percent_encode_path_segment(package_id);
    let path = parsed.path();
    let package_prefix = format!("/packages/{encoded_package_id}/");
    if !path.starts_with(&package_prefix) {
        return false;
    }

    let granule_prefix = format!("/packages/{encoded_package_id}/granules/");
    let Some(expected_granule_id) = granule_id.map(percent_encode_path_segment) else {
        return !path.starts_with(&granule_prefix);
    };
    if !path.starts_with(&granule_prefix) {
        return allows_package_level_for_granule(download_key);
    }

    let tail = &path[granule_prefix.len()..];
    let actual_granule_id = tail.split('/').next().unwrap_or_default();
    actual_granule_id == expected_granule_id
}

fn allows_package_level_for_granule(download_key: &str) -> bool {
    matches!(download_key, "zipLink" | "premisLink")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_prefer_pdf_xml_mods_order() {
        let payload = serde_json::json!({
            "download": {
                "modsLink": "https://api.govinfo.gov/packages/USREPORTS-99/mods",
                "pdfLink": "https://api.govinfo.gov/packages/USREPORTS-99/pdf",
                "txtLink": "https://api.govinfo.gov/packages/USREPORTS-99/txt",
                "xmlLink": "https://api.govinfo.gov/packages/USREPORTS-99/xml"
            }
        });

        let attachments = attachments_from_download(payload.get("download"), "USREPORTS-99", None);
        let urls = attachments
            .iter()
            .map(|asset| asset.asset_url.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            urls,
            vec![
                "https://api.govinfo.gov/packages/USREPORTS-99/pdf",
                "https://api.govinfo.gov/packages/USREPORTS-99/xml",
                "https://api.govinfo.gov/packages/USREPORTS-99/mods",
                "https://api.govinfo.gov/packages/USREPORTS-99/txt"
            ]
        );
        assert_eq!(attachments[0].role, SourceAssetRole::Pdf);
    }

    #[test]
    fn attachments_reject_official_mismatches_and_non_govinfo_hosts() {
        let payload = serde_json::json!({
            "download": {
                "pdfLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/pdf",
                "htmLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/htm",
                "xmlLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg999/xml",
                "txtLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/txt",
                "modsLink": "https://api.govinfo.gov/packages/WRONG-PACKAGE/granules/WCPD-2009-01-19-Pg36/mods",
                "packagePdfLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/pdf",
                "evilTxtLink": "https://evil.example.test/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/txt",
                "zipLink": "https://api.govinfo.gov/packages/WCPD-2009-01-19/zip"
            }
        });

        let attachments = attachments_from_download(
            payload.get("download"),
            "WCPD-2009-01-19",
            Some("WCPD-2009-01-19-Pg36"),
        );
        let urls = attachments
            .iter()
            .map(|asset| asset.asset_url.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            urls,
            vec![
                "https://api.govinfo.gov/packages/WCPD-2009-01-19/granules/WCPD-2009-01-19-Pg36/pdf",
                "https://api.govinfo.gov/packages/WCPD-2009-01-19/zip"
            ]
        );
    }
}
