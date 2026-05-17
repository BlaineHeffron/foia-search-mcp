use crate::sources::{SourceAsset, SourceAssetRole};

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
