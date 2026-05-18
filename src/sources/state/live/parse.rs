use crate::sources::{SourceAssetRole, SourceMetadata, SourceRecord};

use super::asset::{asset_from_link, asset_priority_key, dedupe_assets, is_likely_asset_link};
use super::html::{anchors, clean_html_text, first_tag_text};
use super::url::{
    absolutize, canonicalize_official_url, document_key, is_allowed_state_url, source_id_from_url,
};
use super::{state_citation_note, state_terms_note, STATE_SEARCH_PATH, STATE_SOURCE};

pub(crate) const SOURCE_WARNING: &str = "State Department Virtual Reading Room results are official FOIA leads. The source warns that full-text search depends on OCR, some fields may be unavailable, and some documents may originate with other federal agencies. Page-level citations require ingesting the linked PDF and verifying page boundaries.";

pub(crate) fn records_from_search_page(
    html: &str,
    base_url: &str,
    search_url: &str,
) -> Vec<SourceRecord> {
    let mut records = Vec::new();

    for (href, text) in anchors(html) {
        let url = canonicalize_official_url(&absolutize(&href, search_url), base_url);
        if !is_allowed_state_url(&url, base_url) || is_navigation_label(&text) {
            continue;
        }
        if !looks_like_vrr_lead(&url, &text) {
            continue;
        }

        if is_likely_asset_link(&url, &text) {
            records.push(record_from_direct_asset_link(&url, &text, search_url));
        } else {
            records.push(record_from_page_link(&url, &text, search_url));
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
            if !is_allowed_state_url(&url, base_url) || !is_likely_asset_link(&url, &label) {
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
        title_from_heading(&heading)
    } else {
        title_from_source_id(&source_id)
    };
    let mut metadata = base_metadata(record_url, &source_id, &title);
    metadata.insert("listing_origin".to_owned(), "state_record_page".to_owned());
    metadata.insert("detail_text_preview".to_owned(), preview(&body_text, 320));
    add_asset_metadata(&mut metadata, &attachments);

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    Some(SourceRecord {
        id: format!("{STATE_SOURCE}:{source_id}"),
        document_key: document_key(STATE_SOURCE, &source_id),
        source: STATE_SOURCE,
        source_id,
        title,
        date: metadata.get("document_date").cloned(),
        collection: Some("State Department Virtual Reading Room".to_owned()),
        record_group: Some("state_foia_virtual_reading_room".to_owned()),
        description: Some(description_for_record(pdf_url.is_some()).to_owned()),
        origin_url: format!("{}{}", base_url.trim_end_matches('/'), STATE_SEARCH_PATH),
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: Some(preview(&body_text, 240)).filter(|value| !value.is_empty()),
        citation_note: Some(state_citation_note().to_owned()),
        terms_note: Some(state_terms_note().to_owned()),
    })
}

pub(crate) fn record_from_direct_asset_url(url: &str, base_url: &str) -> SourceRecord {
    record_from_direct_asset_link(
        url,
        "",
        &format!("{}{}", base_url.trim_end_matches('/'), STATE_SEARCH_PATH),
    )
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
        record.text_preview.as_deref().unwrap_or_default(),
    )
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

fn record_from_page_link(url: &str, label: &str, listing_url: &str) -> SourceRecord {
    let source_id = source_id_from_url(url);
    let title = title_for_link(label, &source_id);
    let mut metadata = base_metadata(url, &source_id, &title);
    metadata.insert("listing_origin".to_owned(), listing_url.to_owned());
    add_asset_metadata(&mut metadata, &[]);

    SourceRecord {
        id: format!("{STATE_SOURCE}:{source_id}"),
        document_key: document_key(STATE_SOURCE, &source_id),
        source: STATE_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("State Department Virtual Reading Room".to_owned()),
        record_group: Some("state_foia_virtual_reading_room".to_owned()),
        description: Some(description_for_record(false).to_owned()),
        origin_url: listing_url.to_owned(),
        document_url: url.to_owned(),
        pdf_url: None,
        metadata,
        attachments: Vec::new(),
        text_preview: None,
        citation_note: Some(state_citation_note().to_owned()),
        terms_note: Some(state_terms_note().to_owned()),
    }
}

fn record_from_direct_asset_link(url: &str, label: &str, listing_url: &str) -> SourceRecord {
    let source_id = source_id_from_url(url);
    let title = title_for_link(label, &source_id);
    let asset = asset_from_link(url, &title);
    let attachments = vec![asset.clone()];

    let mut metadata = base_metadata(url, &source_id, &title);
    metadata.insert("listing_origin".to_owned(), listing_url.to_owned());
    add_asset_metadata(&mut metadata, &attachments);

    SourceRecord {
        id: format!("{STATE_SOURCE}:{source_id}"),
        document_key: document_key(STATE_SOURCE, &source_id),
        source: STATE_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("State Department Virtual Reading Room".to_owned()),
        record_group: Some("state_foia_virtual_reading_room".to_owned()),
        description: Some(description_for_record(asset.role == SourceAssetRole::Pdf).to_owned()),
        origin_url: listing_url.to_owned(),
        document_url: url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(state_citation_note().to_owned()),
        terms_note: Some(state_terms_note().to_owned()),
    }
}

fn looks_like_vrr_lead(url: &str, label: &str) -> bool {
    let lower_url = url.to_ascii_lowercase();
    let lower_label = label.to_ascii_lowercase();
    lower_url.contains("/search/")
        || lower_url.contains("/foialibrary/")
        || lower_url.contains("/documents/")
        || lower_url.contains("/docs/")
        || lower_label.contains("case")
        || lower_label.contains("pdf")
        || lower_label.contains("document")
}

fn is_navigation_label(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "" | "foia home"
            | "making a foia request"
            | "foia library"
            | "records management"
            | "mandatory declassification review"
            | "about us/contact us"
            | "site map"
            | "privacy & disclaimers"
            | "foia.gov"
            | "state.gov"
            | "contact"
            | "search released documents"
    )
}

fn description_for_record(has_pdf: bool) -> &'static str {
    if has_pdf {
        "Official State Department FOIA Virtual Reading Room PDF lead. Verify OCR and page boundaries before citation."
    } else {
        "Official State Department FOIA Virtual Reading Room lead. Resolve the official page and prefer linked PDFs for ingestion."
    }
}

fn base_metadata(official_url: &str, source_id: &str, title: &str) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("official_page_url".to_owned(), official_url.to_owned());
    metadata.insert("state_path".to_owned(), source_id.to_owned());
    metadata.insert(
        "collection".to_owned(),
        "State Department Virtual Reading Room".to_owned(),
    );
    metadata.insert("file_title".to_owned(), title.to_owned());
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    if let Some(case_number) = query_value(source_id, "caseNumber") {
        metadata.insert("case_number".to_owned(), case_number);
    }
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
                .join("; "),
        );
    }
}

fn dedupe_records(records: Vec<SourceRecord>) -> Vec<SourceRecord> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for record in records {
        if seen.insert(record.source_id.clone()) {
            deduped.push(record);
        }
    }
    deduped
}

fn title_for_link(label: &str, source_id: &str) -> String {
    let trimmed = label.trim();
    if !trimmed.is_empty() {
        return trimmed.to_owned();
    }
    title_from_source_id(source_id)
}

fn title_from_heading(heading: &str) -> String {
    let trimmed = heading.trim();
    if trimmed.eq_ignore_ascii_case("FOIA Search")
        || trimmed.eq_ignore_ascii_case("Search Released Documents")
    {
        "State Department FOIA Virtual Reading Room Record".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn title_from_source_id(source_id: &str) -> String {
    if let Some(case_number) = query_value(source_id, "caseNumber") {
        return format!("State Department FOIA case {case_number}");
    }

    source_id
        .split(['/', '?', '&', '='])
        .filter(|segment| !segment.trim().is_empty())
        .next_back()
        .map(|segment| segment.replace(['-', '_', '+'], " "))
        .unwrap_or_else(|| "State Department FOIA Virtual Reading Room Record".to_owned())
}

fn first_non_empty(values: &[Option<String>]) -> String {
    values
        .iter()
        .filter_map(|value| value.as_deref())
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_default()
}

fn preview(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn query_value(source_id: &str, key: &str) -> Option<String> {
    let query = source_id.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name.eq_ignore_ascii_case(key) && !value.trim().is_empty()).then(|| value.to_owned())
    })
}
