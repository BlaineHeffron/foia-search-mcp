use crate::sources::{SourceAssetRole, SourceMetadata, SourceRecord};

use super::asset::{asset_from_link, asset_priority_key, dedupe_assets, is_likely_asset_link};
use super::html::{anchors, clean_html_text, first_tag_text};
use super::url::{
    absolutize, canonicalize_official_url, document_key, is_allowed_nsa_url, source_id_from_url,
};
use super::{
    nsa_citation_note, nsa_terms_note, NSA_READING_ROOM_PATH, NSA_REPORTS_LIST_PATH, NSA_SOURCE,
};

pub(crate) const SOURCE_WARNING: &str = "NSA FOIA Reading Room leads are official source leads. Page-level citations require ingesting the linked PDF and verifying page boundaries, redactions, and release context.";

pub(crate) fn records_from_listing_page(
    html: &str,
    base_url: &str,
    listing_url: &str,
) -> Vec<SourceRecord> {
    let mut records = Vec::new();

    for (href, text) in anchors(html) {
        let url = canonicalize_official_url(&absolutize(&href, listing_url), base_url);
        if !is_allowed_nsa_url(&url, base_url) || is_navigation_label(&text) {
            continue;
        }
        if !looks_like_reading_room_lead(&url, &text) {
            continue;
        }

        if is_likely_asset_link(&url, &text) {
            records.push(record_from_direct_asset_link(&url, &text, listing_url));
        } else {
            records.push(record_from_page_link(&url, &text, listing_url));
        }
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
    let body_text = clean_html_text(html);
    let has_heading = !heading.trim().is_empty();
    if !has_heading && body_text.trim().is_empty() {
        return None;
    }

    let mut attachments = anchors(html)
        .into_iter()
        .filter_map(|(href, label)| {
            let url = canonicalize_official_url(&absolutize(&href, record_url), base_url);
            if !is_allowed_nsa_url(&url, base_url) || !is_likely_asset_link(&url, &label) {
                return None;
            }
            Some(asset_from_link(&url, &label))
        })
        .collect::<Vec<_>>();
    attachments = dedupe_assets(attachments);
    attachments.sort_by_key(asset_priority_key);

    if !has_heading && attachments.is_empty() {
        return None;
    }

    let title = if has_heading {
        heading
    } else {
        title_from_source_id(&source_id)
    };
    let category = category_for_url(record_url);
    let collection = collection_for_category(category);
    let mut metadata = base_metadata(record_url, &source_id, &title, category, collection);
    metadata.insert("listing_origin".to_owned(), "nsa_record_page".to_owned());
    add_asset_metadata(&mut metadata, &attachments);

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    Some(SourceRecord {
        id: format!("{NSA_SOURCE}:{source_id}"),
        document_key: document_key(NSA_SOURCE, &source_id),
        source: NSA_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some(collection.to_owned()),
        record_group: Some(category.to_owned()),
        description: Some(description_for_category(category).to_owned()),
        origin_url: format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            NSA_READING_ROOM_PATH
        ),
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(nsa_citation_note().to_owned()),
        terms_note: Some(nsa_terms_note().to_owned()),
    })
}

pub(crate) fn record_from_direct_asset_url(url: &str, base_url: &str) -> SourceRecord {
    record_from_direct_asset_link(
        url,
        "",
        &format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            NSA_READING_ROOM_PATH
        ),
    )
}

pub(crate) fn record_matches_query(record: &SourceRecord, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }

    let haystack = format!(
        "{} {} {} {} {}",
        record.title,
        record.source_id,
        record.document_url,
        record.collection.as_deref().unwrap_or_default(),
        record.record_group.as_deref().unwrap_or_default(),
    )
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

fn record_from_page_link(url: &str, label: &str, listing_url: &str) -> SourceRecord {
    let source_id = source_id_from_url(url);
    let title = title_for_link(label, &source_id);
    let category = category_for_url(url);
    let collection = collection_for_category(category);
    let mut metadata = base_metadata(url, &source_id, &title, category, collection);
    metadata.insert(
        "listing_origin".to_owned(),
        listing_origin_for_url(listing_url).to_owned(),
    );
    add_asset_metadata(&mut metadata, &[]);

    SourceRecord {
        id: format!("{NSA_SOURCE}:{source_id}"),
        document_key: document_key(NSA_SOURCE, &source_id),
        source: NSA_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some(collection.to_owned()),
        record_group: Some(category.to_owned()),
        description: Some(description_for_category(category).to_owned()),
        origin_url: listing_url.to_owned(),
        document_url: url.to_owned(),
        pdf_url: None,
        metadata,
        attachments: Vec::new(),
        text_preview: None,
        citation_note: Some(nsa_citation_note().to_owned()),
        terms_note: Some(nsa_terms_note().to_owned()),
    }
}

fn record_from_direct_asset_link(url: &str, label: &str, listing_url: &str) -> SourceRecord {
    let source_id = source_id_from_url(url);
    let title = title_for_link(label, &source_id);
    let category = category_for_url(url);
    let collection = collection_for_category(category);
    let asset = asset_from_link(url, &title);
    let attachments = vec![asset.clone()];

    let mut metadata = base_metadata(url, &source_id, &title, category, collection);
    metadata.insert(
        "listing_origin".to_owned(),
        listing_origin_for_url(listing_url).to_owned(),
    );
    add_asset_metadata(&mut metadata, &attachments);

    SourceRecord {
        id: format!("{NSA_SOURCE}:{source_id}"),
        document_key: document_key(NSA_SOURCE, &source_id),
        source: NSA_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some(collection.to_owned()),
        record_group: Some(category.to_owned()),
        description: Some(description_for_category(category).to_owned()),
        origin_url: listing_url.to_owned(),
        document_url: url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(nsa_citation_note().to_owned()),
        terms_note: Some(nsa_terms_note().to_owned()),
    }
}

fn looks_like_reading_room_lead(url: &str, label: &str) -> bool {
    let lower_url = url.to_ascii_lowercase();
    let lower_label = label.to_ascii_lowercase();
    lower_url.contains("/helpful-links/nsa-foia/reading-room/")
        || lower_url.contains("/foia-reports-and-releases/")
        || lower_url.contains("/portals/75/documents/")
        || lower_label.contains("foia")
        || lower_label.contains("pdf")
}

fn is_navigation_label(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "" | "learn more" | "enter the room" | "search info" | "browse documents"
    )
}

fn category_for_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("/foia-reports-and-releases/") || lower.contains("/declassified-documents/") {
        "foia_reports_and_releases"
    } else if lower.contains("/foia-handbook") || lower.contains("policy") {
        "reading_room_policy"
    } else {
        "reading_room"
    }
}

fn collection_for_category(category: &str) -> &'static str {
    match category {
        "foia_reports_and_releases" => "NSA FOIA Reports and Releases",
        "reading_room_policy" => "NSA FOIA Reading Room Policy Records",
        _ => "NSA FOIA Reading Room",
    }
}

fn description_for_category(category: &str) -> &'static str {
    match category {
        "foia_reports_and_releases" => {
            "Official NSA FOIA Reports and Releases lead. Prefer linked PDFs and verify page boundaries before citation."
        }
        "reading_room_policy" => {
            "Official NSA FOIA Reading Room policy/manual lead. Cite the official NSA page or linked PDF."
        }
        _ => "Official NSA FOIA Reading Room lead. Follow official NSA page/PDF links for citation.",
    }
}

fn base_metadata(
    official_url: &str,
    source_id: &str,
    title: &str,
    category: &str,
    collection: &str,
) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("official_page_url".to_owned(), official_url.to_owned());
    metadata.insert("nsa_path".to_owned(), format!("/{source_id}"));
    metadata.insert("record_category".to_owned(), category.to_owned());
    metadata.insert("collection".to_owned(), collection.to_owned());
    metadata.insert("file_title".to_owned(), title.to_owned());
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata
}

fn add_asset_metadata(metadata: &mut SourceMetadata, attachments: &[crate::sources::SourceAsset]) {
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
}

fn title_for_link(label: &str, source_id: &str) -> String {
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

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let first = first.to_ascii_uppercase();
    let rest = chars.as_str().to_ascii_lowercase();
    format!("{first}{rest}")
}

fn listing_origin_for_url(url: &str) -> &'static str {
    if url.contains(NSA_REPORTS_LIST_PATH) {
        "foia_reports_and_releases_list"
    } else {
        "reading_room"
    }
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
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
