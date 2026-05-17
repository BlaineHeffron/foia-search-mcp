use std::collections::HashSet;

use crate::sources::{SourceAsset, SourceAssetRole};

pub(crate) fn asset_from_link(url: &str, label: &str) -> SourceAsset {
    asset_from_link_inner(url, label, false)
}

pub(crate) fn metadata_asset_from_partner_link(url: &str, label: &str) -> SourceAsset {
    asset_from_link_inner(url, label, true)
}

fn asset_from_link_inner(url: &str, label: &str, metadata_only: bool) -> SourceAsset {
    let lower = url.to_ascii_lowercase();
    let label_lower = label.to_ascii_lowercase();

    let (role, mime_type) = if lower.ends_with(".pdf")
        || lower.contains(".pdf?")
        || lower.contains("/pdf/")
        || lower.contains("/pdfs/")
    {
        (SourceAssetRole::Pdf, Some("application/pdf".to_owned()))
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        (SourceAssetRole::Image, Some("image/jpeg".to_owned()))
    } else if lower.ends_with(".png") {
        (SourceAssetRole::Image, Some("image/png".to_owned()))
    } else if lower.ends_with(".gif") {
        (SourceAssetRole::Image, Some("image/gif".to_owned()))
    } else if lower.ends_with(".mp4")
        || lower.ends_with(".mov")
        || lower.ends_with(".wmv")
        || lower.contains("dvidshub.net/video")
        || label_lower.contains(" video")
        || label_lower.starts_with("video")
    {
        (SourceAssetRole::Other, Some("video/mp4".to_owned()))
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        (SourceAssetRole::Html, Some("text/html".to_owned()))
    } else {
        (SourceAssetRole::Other, None)
    };

    let (role, mime_type) = if metadata_only {
        (SourceAssetRole::Other, None)
    } else {
        (role, mime_type)
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
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".gif")
        || lower.ends_with(".mp4")
        || lower.ends_with(".mov")
        || lower.ends_with(".wmv")
        || lower.contains("/portals/136/pdfs/")
        || lower.contains("dvidshub.net/video")
        || label_lower.contains("pdf")
        || label_lower.contains("video")
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

pub(crate) fn asset_priority_key(asset: &SourceAsset) -> (u8, u8, u16, String) {
    let priority = match asset.role {
        SourceAssetRole::Pdf => 0,
        SourceAssetRole::Html => 1,
        SourceAssetRole::Image => 2,
        SourceAssetRole::OcrText | SourceAssetRole::Transcript => 3,
        SourceAssetRole::Other => 4,
    };
    let appendix_penalty = if asset.label.to_ascii_lowercase().contains("appendix") {
        1
    } else {
        0
    };

    let label_order = part_number_from_text(&asset.label).unwrap_or(u16::MAX);
    (
        priority,
        appendix_penalty,
        label_order,
        asset.label.to_ascii_lowercase(),
    )
}

fn part_number_from_text(value: &str) -> Option<u16> {
    let lower = value.to_ascii_lowercase();
    let marker = "part";
    let marker_index = lower.find(marker)? + marker.len();

    let digits = lower[marker_index..]
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse::<u16>().ok()
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
