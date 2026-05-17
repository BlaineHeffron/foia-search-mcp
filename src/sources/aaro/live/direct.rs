use crate::sources::{SourceAssetRole, SourceMetadata, SourceRecord};

use super::asset::asset_from_link;
use super::parse::SOURCE_WARNING;
use super::url::{document_key, source_id_from_url};
use super::{aaro_citation_note, aaro_terms_note, AARO_RECORDS_PATH, AARO_SOURCE};

pub(crate) fn record_from_direct_asset_link(
    url: &str,
    label: &str,
    base_url: &str,
    listing_url: &str,
) -> SourceRecord {
    let mut record = record_from_direct_asset_url(url, Some(label), base_url);
    record.origin_url = listing_url.to_owned();
    record
}

pub(crate) fn record_from_direct_asset_url(
    url: &str,
    label: Option<&str>,
    base_url: &str,
) -> SourceRecord {
    let source_id = source_id_from_url(url);
    let title = title_for_asset(label.unwrap_or_default(), &source_id);
    let group = group_from_url(url);
    let collection = collection_for_group(group);
    let description = description_for_group(group);
    let asset = asset_from_link(url, &title);

    let mut metadata = base_metadata(url, &title, group, collection);
    metadata.insert(
        "originating_agency".to_owned(),
        originating_agency(&title).to_owned(),
    );
    metadata.insert("asset_count".to_owned(), "1".to_owned());
    metadata.insert(
        "pdf_asset_count".to_owned(),
        if asset.role == SourceAssetRole::Pdf {
            "1"
        } else {
            "0"
        }
        .to_owned(),
    );
    metadata.insert("asset_labels".to_owned(), asset.label.clone());

    SourceRecord {
        id: format!("{AARO_SOURCE}:{source_id}"),
        document_key: document_key(AARO_SOURCE, &source_id),
        source: AARO_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some(collection.to_owned()),
        record_group: Some(group.to_owned()),
        description: Some(description.to_owned()),
        origin_url: format!("{}{}", base_url.trim_end_matches('/'), AARO_RECORDS_PATH),
        document_url: url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments: vec![asset],
        text_preview: None,
        citation_note: Some(aaro_citation_note().to_owned()),
        terms_note: Some(aaro_terms_note().to_owned()),
    }
}

fn title_for_asset(label: &str, source_id: &str) -> String {
    let label = label.trim();
    if !label.is_empty() {
        return label.to_owned();
    }

    source_id
        .split('/')
        .next_back()
        .unwrap_or(source_id)
        .split(['-', '_'])
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn group_from_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("/uap-cases/") {
        "case_resolution"
    } else if lower.contains("white") || lower.contains("paper") {
        "information_papers"
    } else {
        "records"
    }
}

fn collection_for_group(group: &str) -> &'static str {
    match group {
        "case_resolution" => "AARO UAP Case Resolution Reports",
        "information_papers" => "AARO Information Papers",
        _ => "AARO UAP Records",
    }
}

fn description_for_group(group: &str) -> &'static str {
    match group {
        "case_resolution" => {
            "Official AARO UAP case-resolution release with assessment context and linked assets."
        }
        "information_papers" => {
            "Official AARO information paper or technical note related to UAP analysis and declassification context."
        }
        _ => "Official AARO UAP historical records lead.",
    }
}

fn base_metadata(
    official_page_url: &str,
    event_title: &str,
    group: &str,
    collection: &str,
) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("official_page_url".to_owned(), official_page_url.to_owned());
    metadata.insert("event_title".to_owned(), event_title.to_owned());
    metadata.insert("record_category".to_owned(), group.to_owned());
    metadata.insert("collection".to_owned(), collection.to_owned());
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata
}

fn originating_agency(title: &str) -> &'static str {
    if title.to_ascii_lowercase().contains("kona blue") {
        "Department of Homeland Security (DHS)"
    } else {
        "All-domain Anomaly Resolution Office (AARO)"
    }
}
