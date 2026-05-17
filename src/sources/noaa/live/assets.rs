use crate::sources::{SourceAsset, SourceAssetRole, SourceRecord};

pub(crate) fn dedupe_and_sort_assets(assets: Vec<SourceAsset>) -> Vec<SourceAsset> {
    let mut deduped = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for asset in assets {
        let key = asset.asset_url.trim().to_ascii_lowercase();
        if seen.insert(key) {
            deduped.push(asset);
        }
    }

    deduped.sort_by_key(asset_priority_key);
    deduped
}

pub(crate) fn dedupe_search_records(records: Vec<SourceRecord>) -> Vec<SourceRecord> {
    let mut merged: std::collections::BTreeMap<String, SourceRecord> =
        std::collections::BTreeMap::new();

    for record in records {
        if let Some(existing) = merged.get_mut(&record.source_id) {
            if existing.pdf_url.is_none() && record.pdf_url.is_some() {
                existing.pdf_url = record.pdf_url.clone();
            }
            if existing.description.is_none() && record.description.is_some() {
                existing.description = record.description.clone();
            }
            for (key, value) in record.metadata {
                existing.metadata.entry(key).or_insert(value);
            }
            existing.attachments.extend(record.attachments);
            existing.attachments = dedupe_and_sort_assets(existing.attachments.clone());
            continue;
        }
        merged.insert(record.source_id.clone(), record);
    }

    merged.into_values().collect()
}

fn asset_priority_key(asset: &SourceAsset) -> (u8, String, String) {
    let role_rank = match asset.role {
        SourceAssetRole::Pdf => 0,
        SourceAssetRole::Html => 1,
        _ => 2,
    };

    (
        role_rank,
        asset.label.to_ascii_lowercase(),
        asset.asset_url.to_ascii_lowercase(),
    )
}
