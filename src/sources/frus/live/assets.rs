use crate::sources::{SourceAsset, SourceAssetRole};

pub(crate) fn build_assets(
    official_url: &str,
    tei_url: Option<&str>,
    pdf_url: Option<&str>,
    ebook_url: Option<&str>,
) -> Vec<SourceAsset> {
    let mut assets = Vec::new();

    if let Some(url) = tei_url.filter(|value| !value.trim().is_empty()) {
        assets.push(SourceAsset {
            asset_url: url.to_owned(),
            label: "FRUS TEI/XML".to_owned(),
            mime_type: Some("application/tei+xml".to_owned()),
            role: SourceAssetRole::Transcript,
        });
    }

    if let Some(url) = pdf_url.filter(|value| !value.trim().is_empty()) {
        assets.push(SourceAsset {
            asset_url: url.to_owned(),
            label: "FRUS Volume PDF".to_owned(),
            mime_type: Some("application/pdf".to_owned()),
            role: SourceAssetRole::Pdf,
        });
    }

    if !official_url.trim().is_empty() {
        assets.push(SourceAsset {
            asset_url: official_url.to_owned(),
            label: "FRUS Official Page".to_owned(),
            mime_type: Some("text/html".to_owned()),
            role: SourceAssetRole::Html,
        });
    }

    if let Some(url) = ebook_url.filter(|value| !value.trim().is_empty()) {
        assets.push(SourceAsset {
            asset_url: url.to_owned(),
            label: "FRUS EPUB".to_owned(),
            mime_type: Some("application/epub+zip".to_owned()),
            role: SourceAssetRole::Other,
        });
    }

    dedupe_and_sort_assets(assets)
}

pub(crate) fn dedupe_and_sort_assets(mut assets: Vec<SourceAsset>) -> Vec<SourceAsset> {
    let mut deduped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for asset in assets.drain(..) {
        if seen.insert(asset.asset_url.clone()) {
            deduped.push(asset);
        }
    }

    deduped.sort_by_key(asset_priority_key);
    deduped
}

fn asset_priority_key(asset: &SourceAsset) -> (u8, String) {
    let priority = match asset.role {
        SourceAssetRole::Transcript => 0,
        SourceAssetRole::Pdf => 1,
        SourceAssetRole::Html => 2,
        SourceAssetRole::OcrText => 3,
        SourceAssetRole::Image => 4,
        SourceAssetRole::Other => 5,
    };
    (priority, asset.label.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_prefer_tei_then_pdf_then_html_then_ebook() {
        let assets = build_assets(
            "https://history.state.gov/historicaldocuments/frus1969-76v12/d34",
            Some("https://history.state.gov/historicaldocuments/frus1969-76v12/d34?format=tei"),
            Some("https://static.history.state.gov/frus/frus1969-76v12.pdf"),
            Some("https://history.state.gov/historicaldocuments/frus1969-76v12/epub"),
        );

        assert_eq!(assets.len(), 4);
        assert_eq!(assets[0].role, SourceAssetRole::Transcript);
        assert_eq!(assets[1].role, SourceAssetRole::Pdf);
        assert_eq!(assets[2].role, SourceAssetRole::Html);
        assert_eq!(assets[3].role, SourceAssetRole::Other);
    }
}
