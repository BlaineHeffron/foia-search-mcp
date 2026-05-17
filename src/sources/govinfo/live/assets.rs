use crate::sources::{SourceAsset, SourceAssetRole};
use serde_json::Value;
use std::collections::HashSet;

pub(crate) fn attachments_from_download(download: Option<&Value>) -> Vec<SourceAsset> {
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
            if url.is_empty() {
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
        if url.is_empty() || !seen.insert(url.to_owned()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_prefer_pdf_xml_mods_order() {
        let payload = serde_json::json!({
            "download": {
                "modsLink": "https://api.govinfo.gov/mods",
                "pdfLink": "https://api.govinfo.gov/pdf",
                "txtLink": "https://api.govinfo.gov/txt",
                "xmlLink": "https://api.govinfo.gov/xml"
            }
        });

        let attachments = attachments_from_download(payload.get("download"));
        let urls = attachments
            .iter()
            .map(|asset| asset.asset_url.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            urls,
            vec![
                "https://api.govinfo.gov/pdf",
                "https://api.govinfo.gov/xml",
                "https://api.govinfo.gov/mods",
                "https://api.govinfo.gov/txt"
            ]
        );
        assert_eq!(attachments[0].role, SourceAssetRole::Pdf);
    }
}
