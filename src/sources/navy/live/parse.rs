use crate::sources::{SourceAssetRole, SourceMetadata, SourceRecord};

use super::asset::{asset_from_link, asset_priority_key, dedupe_assets, is_likely_asset_link};
use super::html::{anchors, clean_html_text, first_tag_text, table_rows, CellLink};
use super::scope::{collection_for_url, description_for_url, record_group_for_url};
use super::url::{
    absolutize, canonicalize_official_url, document_key, is_allowed_navy_url, source_id_from_url,
};
use super::{navy_citation_note, navy_terms_note, NAVY_READING_ROOM_PATH, NAVY_SOURCE};

pub(crate) const SOURCE_WARNING: &str = "Department of the Navy FOIA Reading Room results are official secnav.navy.mil leads. Page-level citations require ingesting the linked PDF and verifying page boundaries, OCR quality, redactions, and release context before publication.";

pub(crate) fn records_from_reading_room_page(
    html: &str,
    base_url: &str,
    page_url: &str,
) -> Vec<SourceRecord> {
    let mut records = table_records(html, base_url, page_url);
    records.extend(anchor_records(html, base_url, page_url));
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

    let mut attachments = anchors(html)
        .into_iter()
        .filter_map(|(href, label)| {
            let url = canonicalize_official_url(&absolutize(&href, record_url), base_url);
            if !is_allowed_navy_url(&url, base_url) || !is_likely_asset_link(&url, &label) {
                return None;
            }
            Some(asset_from_link(&url, &label))
        })
        .collect::<Vec<_>>();
    attachments = dedupe_assets(attachments);
    attachments.sort_by_key(asset_priority_key);

    if !has_heading && body_text.trim().is_empty() && attachments.is_empty() {
        return None;
    }

    let title = if has_heading {
        title_from_heading(&heading)
    } else {
        title_from_source_id(&source_id)
    };
    let mut metadata = base_metadata(record_url, &source_id, &title);
    metadata.insert("listing_origin".to_owned(), "navy_record_page".to_owned());
    metadata.insert("detail_text_preview".to_owned(), preview(&body_text, 320));
    add_asset_metadata(&mut metadata, &attachments);

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    Some(SourceRecord {
        id: format!("{NAVY_SOURCE}:{source_id}"),
        document_key: document_key(NAVY_SOURCE, &source_id),
        source: NAVY_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some(collection_for_url(record_url).to_owned()),
        record_group: Some(record_group_for_url(record_url).to_owned()),
        description: Some(description_for_url(record_url, pdf_url.is_some()).to_owned()),
        origin_url: format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            NAVY_READING_ROOM_PATH
        ),
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: Some(preview(&body_text, 240)).filter(|value| !value.is_empty()),
        citation_note: Some(navy_citation_note().to_owned()),
        terms_note: Some(navy_terms_note().to_owned()),
    })
}

pub(crate) fn record_from_direct_asset_url(url: &str, base_url: &str) -> SourceRecord {
    record_from_direct_asset_link(
        url,
        "",
        &format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            NAVY_READING_ROOM_PATH
        ),
        None,
    )
}

pub(crate) fn record_matches_query(record: &SourceRecord, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }

    let haystack = format!(
        "{} {} {} {} {} {} {}",
        record.title,
        record.source_id,
        record.document_url,
        record.collection.as_deref().unwrap_or_default(),
        record.record_group.as_deref().unwrap_or_default(),
        record.description.as_deref().unwrap_or_default(),
        record.text_preview.as_deref().unwrap_or_default(),
    )
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

fn table_records(html: &str, base_url: &str, page_url: &str) -> Vec<SourceRecord> {
    table_rows(html)
        .into_iter()
        .filter_map(|cells| record_from_table_cells(cells, base_url, page_url))
        .collect()
}

fn record_from_table_cells(
    cells: Vec<CellLink>,
    base_url: &str,
    page_url: &str,
) -> Option<SourceRecord> {
    let file_cell = cells.iter().find(|cell| {
        cell.href
            .as_deref()
            .map(|href| is_likely_asset_link(href, &cell.text))
            .unwrap_or(false)
    })?;
    let href = file_cell.href.as_deref()?;
    let url = canonicalize_official_url(&absolutize(href, page_url), base_url);
    if !is_allowed_navy_url(&url, base_url) {
        return None;
    }

    let title = cells
        .first()
        .map(|cell| cell.text.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(file_cell.text.trim());
    let topic = cell_text(&cells, 1);
    let date = cell_text(&cells, 2);
    let office = cell_text(&cells, 3);
    Some(record_from_asset_row(TableRecordFields {
        url: &url,
        title,
        label: &file_cell.text,
        listing_url: page_url,
        topic,
        date,
        office,
    }))
}

fn anchor_records(html: &str, base_url: &str, page_url: &str) -> Vec<SourceRecord> {
    anchors(html)
        .into_iter()
        .filter_map(|(href, label)| {
            let url = canonicalize_official_url(&absolutize(&href, page_url), base_url);
            if !is_allowed_navy_url(&url, base_url)
                || is_navigation_label(&label)
                || !looks_like_reading_room_lead(&url, &label)
            {
                return None;
            }
            Some(record_from_direct_asset_link(&url, &label, page_url, None))
        })
        .collect()
}

struct TableRecordFields<'a> {
    url: &'a str,
    title: &'a str,
    label: &'a str,
    listing_url: &'a str,
    topic: Option<&'a str>,
    date: Option<&'a str>,
    office: Option<&'a str>,
}

fn record_from_asset_row(fields: TableRecordFields<'_>) -> SourceRecord {
    let source_id = source_id_from_url(fields.url);
    let asset = asset_from_link(fields.url, fields.label);
    let attachments = vec![asset.clone()];
    let mut metadata = base_metadata(fields.url, &source_id, fields.title);
    metadata.insert("listing_origin".to_owned(), fields.listing_url.to_owned());
    insert_if_present(&mut metadata, "topic", fields.topic);
    insert_if_present(&mut metadata, "originating_office", fields.office);
    add_asset_metadata(&mut metadata, &attachments);

    SourceRecord {
        id: format!("{NAVY_SOURCE}:{source_id}"),
        document_key: document_key(NAVY_SOURCE, &source_id),
        source: NAVY_SOURCE,
        source_id,
        title: title_for_link(fields.title, &asset.label),
        date: fields.date.map(ToOwned::to_owned),
        collection: Some(collection_for_url(fields.url).to_owned()),
        record_group: Some(record_group_for_url(fields.url).to_owned()),
        description: description_from_fields(fields.topic, fields.office),
        origin_url: fields.listing_url.to_owned(),
        document_url: fields.url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(navy_citation_note().to_owned()),
        terms_note: Some(navy_terms_note().to_owned()),
    }
}

fn record_from_direct_asset_link(
    url: &str,
    label: &str,
    listing_url: &str,
    title_hint: Option<&str>,
) -> SourceRecord {
    let source_id = source_id_from_url(url);
    let title = title_hint
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| title_for_link(label, &source_id));
    let asset = asset_from_link(url, &title);
    let attachments = vec![asset.clone()];
    let mut metadata = base_metadata(url, &source_id, &title);
    metadata.insert("listing_origin".to_owned(), listing_url.to_owned());
    add_asset_metadata(&mut metadata, &attachments);

    SourceRecord {
        id: format!("{NAVY_SOURCE}:{source_id}"),
        document_key: document_key(NAVY_SOURCE, &source_id),
        source: NAVY_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some(collection_for_url(url).to_owned()),
        record_group: Some(record_group_for_url(url).to_owned()),
        description: Some(description_for_url(url, asset.role == SourceAssetRole::Pdf).to_owned()),
        origin_url: listing_url.to_owned(),
        document_url: url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(navy_citation_note().to_owned()),
        terms_note: Some(navy_terms_note().to_owned()),
    }
}

fn looks_like_reading_room_lead(url: &str, label: &str) -> bool {
    let lower_url = url.to_ascii_lowercase();
    let lower_label = label.to_ascii_lowercase();
    lower_url.contains("/foia/readingroom/")
        || lower_url.contains("/foia reading room/")
        || lower_url.contains("/navaudsvc/")
        || lower_url.contains("/ig/")
        || lower_label.contains("foia")
        || lower_label.contains("reading room")
        || lower_label.contains("pdf")
}

fn is_navigation_label(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "" | "home" | "search" | "submit a foia request" | "foia request" | "reading room"
    )
}

fn description_from_fields(topic: Option<&str>, office: Option<&str>) -> Option<String> {
    let parts = [topic, office]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        Some("Official Department of the Navy FOIA reading-room lead.".to_owned())
    } else {
        Some(parts.join("; "))
    }
}

fn base_metadata(url: &str, source_id: &str, title: &str) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("official_url".to_owned(), url.to_owned());
    metadata.insert("source_id".to_owned(), source_id.to_owned());
    metadata.insert("title".to_owned(), title.to_owned());
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata
}

fn add_asset_metadata(metadata: &mut SourceMetadata, attachments: &[crate::sources::SourceAsset]) {
    metadata.insert("asset_count".to_owned(), attachments.len().to_string());
    let pdf_count = attachments
        .iter()
        .filter(|asset| asset.role == SourceAssetRole::Pdf)
        .count();
    metadata.insert("pdf_asset_count".to_owned(), pdf_count.to_string());
}

fn insert_if_present(metadata: &mut SourceMetadata, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert(key.to_owned(), value.to_owned());
    }
}

fn cell_text(cells: &[CellLink], index: usize) -> Option<&str> {
    cells
        .get(index)
        .map(|cell| cell.text.trim())
        .filter(|value| !value.is_empty())
}

fn first_non_empty(values: &[Option<String>]) -> String {
    values
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_owned()
}

fn title_for_link(label: &str, fallback: &str) -> String {
    let label = label.trim();
    if !label.is_empty() && !label.eq_ignore_ascii_case("pdf") {
        return label.to_owned();
    }
    title_from_source_id(fallback)
}

fn title_from_heading(heading: &str) -> String {
    heading
        .trim()
        .trim_end_matches(" - Department of the Navy")
        .trim_end_matches(" - Naval Audit Service")
        .to_owned()
}

fn title_from_source_id(source_id: &str) -> String {
    source_id
        .split('/')
        .next_back()
        .unwrap_or(source_id)
        .split(['?', '#'])
        .next()
        .unwrap_or(source_id)
        .replace("%20", " ")
        .replace('_', " ")
        .trim_end_matches(".pdf")
        .trim_end_matches(".PDF")
        .trim()
        .to_owned()
}

fn preview(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn dedupe_records(records: Vec<SourceRecord>) -> Vec<SourceRecord> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for record in records {
        if seen.insert(record.id.clone()) {
            deduped.push(record);
        }
    }
    deduped
}
