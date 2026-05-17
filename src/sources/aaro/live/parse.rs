use crate::sources::{SourceAssetRole, SourceMetadata, SourceRecord};

use super::asset::{
    asset_from_link, asset_priority_key, dedupe_assets, is_likely_asset_link,
    metadata_asset_from_partner_link,
};
use super::direct::record_from_direct_asset_link;
use super::html::{anchors, clean_html_text, first_tag_text, meta_content};
use super::url::{
    absolutize, canonicalize_official_url, document_key, is_allowed_aaro_url,
    is_allowed_partner_asset_url, source_id_from_url,
};
use super::{aaro_citation_note, aaro_terms_note, AARO_RECORDS_PATH, AARO_SOURCE};

pub(crate) const SOURCE_WARNING: &str = "AARO UAP historical records and case releases can include mixed provenance from partner agencies and mixed media assets. Cite official AARO page/PDF URLs and verify release context, redactions, and page boundaries before publication.";

pub(crate) fn records_from_listing_page(
    html: &str,
    base_url: &str,
    listing_url: &str,
) -> Vec<SourceRecord> {
    let mut records = Vec::new();

    for (href, text) in anchors(html) {
        let document_url = canonicalize_official_url(&absolutize(&href, base_url), base_url);
        if !is_allowed_aaro_url(&document_url, base_url) {
            continue;
        }
        if is_likely_asset_link(&document_url, &text) {
            records.push(record_from_direct_asset_link(
                &document_url,
                &text,
                base_url,
                listing_url,
            ));
            continue;
        }
        if !looks_like_record_link(&document_url, &text) {
            continue;
        }

        let source_id = source_id_from_url(&document_url);
        let title = title_for_listing(&text, &source_id);
        let group = group_from_url(&document_url);
        let collection = collection_for_group(group);
        let description = description_for_group(group);

        let mut metadata = base_metadata(&document_url, &title, group, collection);
        metadata.insert(
            "originating_agency".to_owned(),
            infer_originating_agency(&title, "").to_owned(),
        );
        if title.to_ascii_lowercase().contains("case resolution") {
            metadata.insert(
                "review_note".to_owned(),
                "AARO case resolution release; review the official methodology and linked media context before citing conclusions."
                    .to_owned(),
            );
        }

        records.push(SourceRecord {
            id: format!("{AARO_SOURCE}:{source_id}"),
            document_key: document_key(AARO_SOURCE, &source_id),
            source: AARO_SOURCE,
            source_id,
            title,
            date: None,
            collection: Some(collection.to_owned()),
            record_group: Some(group.to_owned()),
            description: Some(description.to_owned()),
            origin_url: listing_url.to_owned(),
            document_url,
            pdf_url: None,
            metadata,
            attachments: Vec::new(),
            text_preview: None,
            citation_note: Some(aaro_citation_note().to_owned()),
            terms_note: Some(aaro_terms_note().to_owned()),
        });
    }

    dedupe_records(records)
}

pub(crate) fn record_from_detail_page(
    html: &str,
    base_url: &str,
    record_url: &str,
    source_id_hint: Option<&str>,
) -> Option<SourceRecord> {
    let source_id = source_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| source_id_from_url(record_url));

    let heading = first_non_empty(&[
        first_tag_text(html, "h1"),
        first_tag_text(html, "h2"),
        first_tag_text(html, "title"),
    ]);
    let has_heading = !heading.trim().is_empty();
    let body_text = clean_html_text(html);
    let title = if !has_heading {
        title_from_source_id(&source_id)
    } else {
        heading
    };

    let mut attachments = anchors(html)
        .into_iter()
        .filter_map(|(href, label)| {
            let url = canonicalize_official_url(&absolutize(&href, record_url), base_url);
            let is_same_origin = is_allowed_aaro_url(&url, base_url);
            let is_partner_metadata =
                is_allowed_partner_asset_url(&url) && is_likely_asset_link(&url, &label);
            if !is_same_origin && !is_partner_metadata {
                return None;
            }
            if !is_likely_asset_link(&url, &label) {
                return None;
            }
            Some(if is_same_origin {
                asset_from_link(&url, &label)
            } else {
                metadata_asset_from_partner_link(&url, &label)
            })
        })
        .collect::<Vec<_>>();

    attachments = dedupe_assets(attachments);
    attachments.sort_by_key(asset_priority_key);

    if (!has_heading && attachments.is_empty()) || body_text.trim().is_empty() {
        return None;
    }

    let group = group_from_url(record_url);
    let collection = collection_for_group(group);
    let description = description_for_group(group);

    let mut metadata = base_metadata(record_url, &title, group, collection);
    metadata.insert(
        "originating_agency".to_owned(),
        meta_content(html, "aaro-originating-agency")
            .unwrap_or_else(|| infer_originating_agency(&title, &body_text).to_owned()),
    );
    if let Some(release_note) = meta_content(html, "aaro-release-note") {
        metadata.insert("release_note".to_owned(), release_note);
    }
    if let Some(review_note) = meta_content(html, "aaro-review-note") {
        metadata.insert("review_note".to_owned(), review_note);
    }
    metadata.insert("asset_count".to_owned(), attachments.len().to_string());
    metadata.insert(
        "pdf_asset_count".to_owned(),
        attachments
            .iter()
            .filter(|asset| asset.role == SourceAssetRole::Pdf)
            .count()
            .to_string(),
    );

    if !attachments.is_empty() {
        metadata.insert(
            "asset_labels".to_owned(),
            attachments
                .iter()
                .map(|asset| asset.label.as_str())
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }

    let pdf_url = attachments
        .iter()
        .find(|asset| {
            asset.role == SourceAssetRole::Pdf
                && !asset.label.to_ascii_lowercase().contains("appendix")
        })
        .or_else(|| {
            attachments
                .iter()
                .find(|asset| asset.role == SourceAssetRole::Pdf)
        })
        .map(|asset| asset.asset_url.clone());

    Some(SourceRecord {
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
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(aaro_citation_note().to_owned()),
        terms_note: Some(aaro_terms_note().to_owned()),
    })
}

pub(crate) fn record_matches_query(record: &SourceRecord, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }

    let haystack = format!(
        "{} {} {} {} {} {}",
        record.title,
        record.source_id,
        record.document_url,
        record.collection.as_deref().unwrap_or_default(),
        record.record_group.as_deref().unwrap_or_default(),
        record
            .metadata
            .get("originating_agency")
            .map(String::as_str)
            .unwrap_or_default(),
    )
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

fn looks_like_record_link(url: &str, label: &str) -> bool {
    if !url.contains("/UAP-") {
        return false;
    }
    if is_likely_asset_link(url, label) {
        return false;
    }

    let label_lower = label.to_ascii_lowercase();
    label_lower.contains("uap")
        || label_lower.contains("case resolution")
        || label_lower.contains("paper")
        || label_lower.contains("kona blue")
        || label_lower.contains("analysis")
}

fn title_for_listing(label: &str, source_id: &str) -> String {
    let label = label.trim();
    if label.is_empty() {
        title_from_source_id(source_id)
    } else {
        label.to_owned()
    }
}

fn title_from_source_id(source_id: &str) -> String {
    source_id
        .split('/')
        .next_back()
        .unwrap_or(source_id)
        .split(['-', '_'])
        .filter(|part| !part.trim().is_empty())
        .map(title_case)
        .collect::<Vec<_>>()
        .join(" ")
}

fn group_from_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("/uap-cases/") {
        "case_resolution"
    } else if lower.contains("information") || lower.contains("white-paper") {
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

fn infer_originating_agency(title: &str, body_text: &str) -> &'static str {
    let haystack = format!("{title} {body_text}").to_ascii_lowercase();
    if haystack.contains("department of homeland security") || haystack.contains("kona blue") {
        "Department of Homeland Security (DHS)"
    } else if haystack.contains("national archives") || haystack.contains("nara") {
        "National Archives and Records Administration (NARA)"
    } else if haystack.contains("nasa") {
        "National Aeronautics and Space Administration (NASA)"
    } else if haystack.contains("oak ridge") || haystack.contains("ornl") {
        "Oak Ridge National Laboratory (ORNL)"
    } else {
        "All-domain Anomaly Resolution Office (AARO)"
    }
}

fn dedupe_records(records: Vec<SourceRecord>) -> Vec<SourceRecord> {
    let mut deduped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for record in records {
        if seen.insert(record.source_id.clone()) {
            deduped.push(record);
        }
    }
    deduped
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let first = first.to_ascii_uppercase();
    let rest = chars.as_str().to_ascii_lowercase();
    format!("{first}{rest}")
}
