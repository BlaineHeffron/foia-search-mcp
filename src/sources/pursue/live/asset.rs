use std::collections::{BTreeMap, HashSet};

use crate::sources::{SourceAsset, SourceAssetRole, SourceRecord};

use super::url::{absolutize, document_key};
use super::{CITATION_NOTE, PURSUE_INDEX_PATH, PURSUE_SOURCE, TERMS_NOTE};

pub(crate) fn ensure_asset_present(assets: &mut Vec<SourceAsset>, asset_url: &str) {
    if assets.iter().any(|asset| asset.asset_url == asset_url) {
        return;
    }

    assets.push(asset_from_url(asset_url));
    assets.sort_by_key(asset_priority_key);
    *assets = dedupe_assets(std::mem::take(assets));
}

pub(crate) fn single_asset_record(asset_url: &str, base_url: &str) -> SourceRecord {
    let asset = asset_from_url(asset_url);
    let source_id = sanitize_id_component(asset.label.as_str());
    let source_id = if source_id.is_empty() {
        "asset".to_owned()
    } else {
        source_id
    };

    let mut metadata = BTreeMap::new();
    metadata.insert("release_tranche".to_owned(), "unknown".to_owned());

    SourceRecord {
        id: format!("{PURSUE_SOURCE}:{source_id}"),
        document_key: document_key(PURSUE_SOURCE, &source_id),
        source: PURSUE_SOURCE,
        source_id,
        title: format!("PURSUE asset {}", asset.label),
        date: None,
        collection: Some("PURSUE".to_owned()),
        record_group: None,
        description: None,
        origin_url: absolutize(PURSUE_INDEX_PATH, base_url),
        document_url: asset_url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments: vec![asset],
        text_preview: None,
        citation_note: Some(CITATION_NOTE.to_owned()),
        terms_note: Some(TERMS_NOTE.to_owned()),
    }
}

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
    } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        (SourceAssetRole::Image, Some("image/tiff".to_owned()))
    } else if lower.ends_with(".mp4") {
        (SourceAssetRole::Other, Some("video/mp4".to_owned()))
    } else if lower.ends_with(".mov") {
        (SourceAssetRole::Other, Some("video/quicktime".to_owned()))
    } else if lower.ends_with(".webm") {
        (SourceAssetRole::Other, Some("video/webm".to_owned()))
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
        SourceAssetRole::OcrText => 4,
        SourceAssetRole::Transcript => 5,
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

pub(crate) fn sanitize_id_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn label_from_url(url: &str) -> String {
    url.split('/')
        .next_back()
        .and_then(|segment| segment.split(['?', '#']).next())
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "asset".to_owned())
}
