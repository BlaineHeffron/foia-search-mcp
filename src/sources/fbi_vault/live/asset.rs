use std::collections::HashSet;

use crate::sources::{SourceAsset, SourceAssetRole};

pub(crate) fn asset_from_link(url: &str, label: &str) -> SourceAsset {
    let normalized_url = normalize_asset_url(url, label);
    let lower = normalized_url.to_ascii_lowercase();

    let (role, mime_type) = if lower.ends_with(".pdf")
        || lower.contains(".pdf?")
        || lower.ends_with("/at_download/file")
    {
        (SourceAssetRole::Pdf, Some("application/pdf".to_owned()))
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        (SourceAssetRole::Image, Some("image/jpeg".to_owned()))
    } else if lower.ends_with(".png") {
        (SourceAssetRole::Image, Some("image/png".to_owned()))
    } else if lower.ends_with(".gif") {
        (SourceAssetRole::Image, Some("image/gif".to_owned()))
    } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        (SourceAssetRole::Image, Some("image/tiff".to_owned()))
    } else if lower.ends_with(".html") || lower.ends_with(".htm") || lower.ends_with("/view") {
        (SourceAssetRole::Html, Some("text/html".to_owned()))
    } else {
        (SourceAssetRole::Other, None)
    };

    let asset_label = label_for_asset(&normalized_url, label);
    SourceAsset {
        asset_url: normalized_url,
        label: asset_label,
        mime_type,
        role,
    }
}

pub(crate) fn normalize_asset_url(url: &str, label: &str) -> String {
    let lower = url.to_ascii_lowercase();
    let label_lower = label.to_ascii_lowercase();
    if lower.ends_with("/view") && looks_like_part_label(&label_lower) {
        let stripped = url.trim_end_matches('/').trim_end_matches("view");
        format!("{stripped}at_download/file")
    } else {
        url.to_owned()
    }
}

pub(crate) fn asset_priority_key(asset: &SourceAsset) -> (u8, u16, String) {
    let priority = match asset.role {
        SourceAssetRole::Pdf => 0,
        SourceAssetRole::Html => 1,
        SourceAssetRole::Image => 2,
        SourceAssetRole::OcrText | SourceAssetRole::Transcript => 3,
        SourceAssetRole::Other => 4,
    };

    let part_order = part_number_from_text(&asset.label)
        .or_else(|| part_number_from_text(&asset.asset_url))
        .unwrap_or(u16::MAX);

    (priority, part_order, asset.label.to_ascii_lowercase())
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

pub(crate) fn part_number_from_text(value: &str) -> Option<u16> {
    let lower = value.to_ascii_lowercase();
    let marker = "part";
    let index = lower.find(marker)? + marker.len();

    let digits = lower[index..]
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse::<u16>().ok()
}

pub(crate) fn looks_like_part_label(lower_label: &str) -> bool {
    lower_label.contains("part")
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
