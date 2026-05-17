use std::collections::HashSet;

use crate::sources::{SourceAsset, SourceAssetRole};

pub(crate) fn asset_from_url(url: &str) -> SourceAsset {
    let lower = url.to_ascii_lowercase();
    let (role, mime_type) = if lower.ends_with(".pdf") {
        (SourceAssetRole::Pdf, Some("application/pdf".to_owned()))
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        (SourceAssetRole::Image, Some("image/jpeg".to_owned()))
    } else if lower.ends_with(".png") {
        (SourceAssetRole::Image, Some("image/png".to_owned()))
    } else if lower.ends_with(".gif") {
        (SourceAssetRole::Image, Some("image/gif".to_owned()))
    } else if lower.ends_with(".webp") {
        (SourceAssetRole::Image, Some("image/webp".to_owned()))
    } else if lower.ends_with(".svg") {
        (SourceAssetRole::Image, Some("image/svg+xml".to_owned()))
    } else if lower.ends_with(".mp4") {
        (SourceAssetRole::Other, Some("video/mp4".to_owned()))
    } else if lower.ends_with(".mov") {
        (SourceAssetRole::Other, Some("video/quicktime".to_owned()))
    } else if lower.ends_with(".webm") {
        (SourceAssetRole::Other, Some("video/webm".to_owned()))
    } else if lower.ends_with(".mp3") {
        (SourceAssetRole::Other, Some("audio/mpeg".to_owned()))
    } else if lower.ends_with(".wav") {
        (SourceAssetRole::Other, Some("audio/wav".to_owned()))
    } else if lower.ends_with(".m4a") {
        (SourceAssetRole::Other, Some("audio/mp4".to_owned()))
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
        SourceAssetRole::Image => 1,
        SourceAssetRole::Other => 2,
        SourceAssetRole::Html => 3,
        SourceAssetRole::Transcript => 4,
        SourceAssetRole::OcrText => 5,
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
        SourceAssetRole::Image => "image",
        SourceAssetRole::Html => "html",
        SourceAssetRole::Transcript => "transcript",
        SourceAssetRole::OcrText => "ocr_text",
        SourceAssetRole::Other => {
            if asset
                .mime_type
                .as_deref()
                .unwrap_or_default()
                .starts_with("video/")
            {
                "video"
            } else if asset
                .mime_type
                .as_deref()
                .unwrap_or_default()
                .starts_with("audio/")
            {
                "audio"
            } else {
                "other"
            }
        }
    }
}

fn label_from_url(url: &str) -> String {
    url.split('/')
        .next_back()
        .and_then(|segment| segment.split(['?', '#']).next())
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "asset".to_owned())
}
