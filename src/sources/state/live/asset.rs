use std::collections::HashSet;

use crate::sources::{SourceAsset, SourceAssetRole};

pub(crate) fn asset_from_link(url: &str, label: &str) -> SourceAsset {
    let lower = url.to_ascii_lowercase();
    let label_lower = label.to_ascii_lowercase();

    let (role, mime_type) = if lower.ends_with(".pdf") || lower.contains(".pdf?") {
        (SourceAssetRole::Pdf, Some("application/pdf".to_owned()))
    } else if lower.ends_with(".txt") || lower.contains(".txt?") {
        (SourceAssetRole::OcrText, Some("text/plain".to_owned()))
    } else if lower.ends_with(".html")
        || lower.ends_with(".htm")
        || lower.contains("/search/")
        || lower.contains("/foialibrary/")
    {
        (SourceAssetRole::Html, Some("text/html".to_owned()))
    } else if label_lower.contains("pdf") {
        (SourceAssetRole::Pdf, Some("application/pdf".to_owned()))
    } else {
        (SourceAssetRole::Other, None)
    };

    SourceAsset {
        asset_url: url.to_owned(),
        label: label_for_asset(url, label),
        mime_type,
        role,
    }
}

pub(crate) fn is_likely_asset_link(url: &str, label: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let label_lower = label.to_ascii_lowercase();
    lower.ends_with(".pdf")
        || lower.contains(".pdf?")
        || lower.ends_with(".txt")
        || lower.contains(".txt?")
        || lower.contains("/documents/")
        || lower.contains("/docs/")
        || label_lower.contains("pdf")
        || label_lower.contains("preview")
        || label_lower.contains("download")
}

pub(crate) fn asset_priority_key(asset: &SourceAsset) -> (u8, String) {
    let priority = match asset.role {
        SourceAssetRole::Pdf => 0,
        SourceAssetRole::Html => 1,
        SourceAssetRole::OcrText | SourceAssetRole::Transcript => 2,
        SourceAssetRole::Image => 3,
        SourceAssetRole::Other => 4,
    };
    (priority, asset.label.to_ascii_lowercase())
}

pub(crate) fn dedupe_assets(assets: Vec<SourceAsset>) -> Vec<SourceAsset> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for asset in assets {
        if seen.insert(asset.asset_url.clone()) {
            deduped.push(asset);
        }
    }
    deduped
}

fn label_for_asset(url: &str, raw_label: &str) -> String {
    let label = raw_label.trim();
    if !label.is_empty() {
        return label.to_owned();
    }

    url.split('/')
        .next_back()
        .and_then(|segment| segment.split(['?', '#']).next())
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "asset".to_owned())
}
