use std::collections::HashSet;

use crate::sources::{SourceAsset, SourceAssetRole};

pub(crate) fn asset_from_url(url: &str) -> SourceAsset {
    let lower = url.to_ascii_lowercase();
    let (role, mime_type) = if looks_like_pdf(&lower) {
        (SourceAssetRole::Pdf, Some("application/pdf".to_owned()))
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        (SourceAssetRole::Html, Some("text/html".to_owned()))
    } else if lower.ends_with(".txt") {
        (SourceAssetRole::OcrText, Some("text/plain".to_owned()))
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        (SourceAssetRole::Image, Some("image/jpeg".to_owned()))
    } else if lower.ends_with(".png") {
        (SourceAssetRole::Image, Some("image/png".to_owned()))
    } else {
        (SourceAssetRole::Other, None)
    };

    SourceAsset {
        asset_url: url.to_owned(),
        label: label_from_url(url),
        mime_type,
        role,
    }
}

pub(crate) fn asset_priority_key(asset: &SourceAsset) -> (u8, String) {
    let priority = match asset.role {
        SourceAssetRole::Pdf => 0,
        SourceAssetRole::Html => 1,
        SourceAssetRole::OcrText | SourceAssetRole::Transcript => 2,
        SourceAssetRole::Other => 3,
        SourceAssetRole::Image => 4,
    };
    (priority, asset.label.clone())
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

pub(crate) fn media_type_for_asset(asset: &SourceAsset) -> &'static str {
    match asset.role {
        SourceAssetRole::Pdf => "pdf",
        SourceAssetRole::Html => "html",
        SourceAssetRole::OcrText => "ocr_text",
        SourceAssetRole::Transcript => "transcript",
        SourceAssetRole::Image => "image",
        SourceAssetRole::Other => "other",
    }
}

fn looks_like_pdf(lower_url: &str) -> bool {
    lower_url.ends_with(".pdf") || lower_url.contains(".pdf?")
}

fn label_from_url(url: &str) -> String {
    url.split('/')
        .next_back()
        .and_then(|segment| segment.split(['?', '#']).next())
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "asset".to_owned())
}
