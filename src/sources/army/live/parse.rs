use crate::sources::{SourceAssetRole, SourceMetadata, SourceRecord};

use super::asset::{asset_from_link, asset_priority_key, dedupe_assets, is_likely_asset_link};
use super::html::{anchors, clean_html_text, first_tag_text, table_rows, CellLink};
use super::url::{
    absolutize, canonicalize_official_url, document_key, is_allowed_army_url, source_id_from_url,
};
use super::{army_citation_note, army_terms_note, ARMY_READING_ROOM_PATH, ARMY_SOURCE};

pub(crate) const SOURCE_WARNING: &str = "Army FOIA Reading Room results are official foia.army.mil leads. Page-level citations require ingesting the linked PDF and verifying page boundaries, OCR quality, redactions, and any document-origin context before publication.";

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
            if !is_allowed_army_url(&url, base_url) || !is_likely_asset_link(&url, &label) {
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
    metadata.insert("listing_origin".to_owned(), "army_record_page".to_owned());
    metadata.insert("detail_text_preview".to_owned(), preview(&body_text, 320));
    add_asset_metadata(&mut metadata, &attachments);

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    Some(SourceRecord {
        id: format!("{ARMY_SOURCE}:{source_id}"),
        document_key: document_key(ARMY_SOURCE, &source_id),
        source: ARMY_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("Army FOIA Reading Room".to_owned()),
        record_group: Some("army_foia_reading_room".to_owned()),
        description: Some(description_for_record(pdf_url.is_some()).to_owned()),
        origin_url: format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            ARMY_READING_ROOM_PATH
        ),
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: Some(preview(&body_text, 240)).filter(|value| !value.is_empty()),
        citation_note: Some(army_citation_note().to_owned()),
        terms_note: Some(army_terms_note().to_owned()),
    })
}

pub(crate) fn record_from_direct_asset_url(url: &str, base_url: &str) -> SourceRecord {
    record_from_direct_asset_link(
        url,
        "",
        &format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            ARMY_READING_ROOM_PATH
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
            .map(|href| href.to_ascii_lowercase().contains("/home/doccontent/"))
            .unwrap_or(false)
    })?;
    let href = file_cell.href.as_deref()?;
    let url = canonicalize_official_url(&absolutize(href, page_url), base_url);
    if !is_allowed_army_url(&url, base_url) {
        return None;
    }

    let title = cells
        .first()
        .map(|cell| cell.text.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(file_cell.text.trim());
    let subject = cell_text(&cells, 1);
    let keywords = cell_text(&cells, 2);
    let date = cell_text(&cells, 3);
    let originator = cell_text(&cells, 4);
    Some(record_from_asset_row(TableRecordFields {
        url: &url,
        title,
        label: &file_cell.text,
        listing_url: page_url,
        subject,
        keywords,
        date,
        originator,
    }))
}

fn anchor_records(html: &str, base_url: &str, page_url: &str) -> Vec<SourceRecord> {
    anchors(html)
        .into_iter()
        .filter_map(|(href, label)| {
            let url = canonicalize_official_url(&absolutize(&href, page_url), base_url);
            if !is_allowed_army_url(&url, base_url)
                || is_navigation_label(&label)
                || !is_likely_asset_link(&url, &label)
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
    subject: Option<&'a str>,
    keywords: Option<&'a str>,
    date: Option<&'a str>,
    originator: Option<&'a str>,
}

fn record_from_asset_row(fields: TableRecordFields<'_>) -> SourceRecord {
    let source_id = source_id_from_url(fields.url);
    let asset = asset_from_link(fields.url, fields.label);
    let attachments = vec![asset.clone()];
    let mut metadata = base_metadata(fields.url, &source_id, fields.title);
    metadata.insert("listing_origin".to_owned(), fields.listing_url.to_owned());
    insert_if_present(&mut metadata, "subject", fields.subject);
    insert_if_present(&mut metadata, "keywords", fields.keywords);
    insert_if_present(&mut metadata, "originator", fields.originator);
    add_asset_metadata(&mut metadata, &attachments);

    SourceRecord {
        id: format!("{ARMY_SOURCE}:{source_id}"),
        document_key: document_key(ARMY_SOURCE, &source_id),
        source: ARMY_SOURCE,
        source_id,
        title: title_for_link(fields.title, &asset.label),
        date: fields.date.map(ToOwned::to_owned),
        collection: Some("Army FOIA Reading Room".to_owned()),
        record_group: Some("army_foia_reading_room".to_owned()),
        description: description_from_fields(fields.subject, fields.keywords, fields.originator),
        origin_url: fields.listing_url.to_owned(),
        document_url: fields.url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(army_citation_note().to_owned()),
        terms_note: Some(army_terms_note().to_owned()),
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
        id: format!("{ARMY_SOURCE}:{source_id}"),
        document_key: document_key(ARMY_SOURCE, &source_id),
        source: ARMY_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("Army FOIA Reading Room".to_owned()),
        record_group: Some("army_foia_reading_room".to_owned()),
        description: Some(description_for_record(asset.role == SourceAssetRole::Pdf).to_owned()),
        origin_url: listing_url.to_owned(),
        document_url: url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(army_citation_note().to_owned()),
        terms_note: Some(army_terms_note().to_owned()),
    }
}

fn is_navigation_label(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "" | "home" | "search" | "requested records" | "foia.gov" | "usa.gov"
    )
}

fn base_metadata(url: &str, source_id: &str, title: &str) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata.insert("source_url".to_owned(), url.to_owned());
    metadata.insert("source_id".to_owned(), source_id.to_owned());
    metadata.insert("title".to_owned(), title.to_owned());
    metadata.insert("official_source".to_owned(), "foia.army.mil".to_owned());
    metadata
}

fn add_asset_metadata(metadata: &mut SourceMetadata, assets: &[crate::sources::SourceAsset]) {
    let pdf_count = assets
        .iter()
        .filter(|asset| asset.role == SourceAssetRole::Pdf)
        .count();
    metadata.insert("asset_count".to_owned(), assets.len().to_string());
    metadata.insert("pdf_asset_count".to_owned(), pdf_count.to_string());
}

fn description_from_fields(
    subject: Option<&str>,
    keywords: Option<&str>,
    originator: Option<&str>,
) -> Option<String> {
    let parts = [subject, keywords, originator]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        Some(description_for_record(true).to_owned())
    } else {
        Some(parts.join(" | "))
    }
}

fn description_for_record(has_pdf: bool) -> &'static str {
    if has_pdf {
        "Official Army FOIA Reading Room lead with an ingest-preferred PDF asset."
    } else {
        "Official Army FOIA Reading Room lead; inspect linked assets before ingestion."
    }
}

fn title_for_link(label: &str, fallback: &str) -> String {
    let title = label.trim();
    if !title.is_empty() {
        return title.to_owned();
    }
    title_from_source_id(fallback)
}

fn title_from_heading(heading: &str) -> String {
    heading
        .trim()
        .trim_start_matches("FOIA Reading Room")
        .trim_matches(['|', '-', ' '])
        .to_owned()
}

fn title_from_source_id(source_id: &str) -> String {
    source_id
        .split('/')
        .next_back()
        .unwrap_or(source_id)
        .replace(['_', '-'], " ")
}

fn first_non_empty(values: &[Option<String>]) -> String {
    values
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned()
}

fn cell_text(cells: &[CellLink], index: usize) -> Option<&str> {
    cells
        .get(index)
        .map(|cell| cell.text.trim())
        .filter(|value| !value.is_empty())
}

fn insert_if_present(metadata: &mut SourceMetadata, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert(key.to_owned(), value.to_owned());
    }
}

fn preview(text: &str, limit: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= limit {
        normalized
    } else {
        format!("{}...", normalized.chars().take(limit).collect::<String>())
    }
}

fn dedupe_records(records: Vec<SourceRecord>) -> Vec<SourceRecord> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for record in records {
        if seen.insert(record.document_url.clone()) {
            deduped.push(record);
        }
    }
    deduped
}
