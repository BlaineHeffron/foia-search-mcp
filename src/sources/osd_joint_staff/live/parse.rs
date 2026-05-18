use crate::sources::{SourceAssetRole, SourceMetadata, SourceRecord};

use super::asset::{asset_from_link, asset_priority_key, dedupe_assets, is_likely_asset_link};
use super::html::{anchors, clean_html_text, first_tag_text};
use super::url::{
    absolutize, canonicalize_official_url, document_key, is_allowed_osd_joint_staff_url,
    source_id_from_url,
};
use super::{
    osd_joint_staff_citation_note, osd_joint_staff_terms_note, OSD_JOINT_STAFF_READING_ROOM_PATH,
    OSD_JOINT_STAFF_SOURCE,
};

pub(crate) const SOURCE_WARNING: &str = "OSD/Joint Staff FOIA Reading Room results are official WHS/ESD OSD/Joint Staff FOIA leads. Page-level citations require ingesting the linked PDF and verifying page boundaries, OCR quality, redactions, and any document-origin context before publication.";

pub(crate) fn records_from_reading_room_page(
    html: &str,
    base_url: &str,
    reading_room_url: &str,
) -> Vec<SourceRecord> {
    let mut records = Vec::new();

    for (href, text) in anchors(html) {
        let url = canonicalize_official_url(&absolutize(&href, reading_room_url), base_url);
        if !is_allowed_osd_joint_staff_url(&url, base_url) || is_navigation_label(&text) {
            continue;
        }
        if !looks_like_reading_room_lead(&url, &text) {
            continue;
        }

        if is_likely_asset_link(&url, &text) {
            records.push(record_from_direct_asset_link(&url, &text, reading_room_url));
        } else {
            records.push(record_from_page_link(&url, &text, reading_room_url));
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
            if !is_allowed_osd_joint_staff_url(&url, base_url)
                || !is_likely_asset_link(&url, &label)
            {
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
    metadata.insert(
        "listing_origin".to_owned(),
        "osd_joint_staff_record_page".to_owned(),
    );
    metadata.insert("detail_text_preview".to_owned(), preview(&body_text, 320));
    add_asset_metadata(&mut metadata, &attachments);

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    Some(SourceRecord {
        id: format!("{OSD_JOINT_STAFF_SOURCE}:{source_id}"),
        document_key: document_key(OSD_JOINT_STAFF_SOURCE, &source_id),
        source: OSD_JOINT_STAFF_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("OSD/Joint Staff FOIA Reading Room".to_owned()),
        record_group: Some("osd_joint_staff_foia_reading_room".to_owned()),
        description: Some(description_for_record(pdf_url.is_some()).to_owned()),
        origin_url: format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            OSD_JOINT_STAFF_READING_ROOM_PATH
        ),
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: Some(preview(&body_text, 240)).filter(|value| !value.is_empty()),
        citation_note: Some(osd_joint_staff_citation_note().to_owned()),
        terms_note: Some(osd_joint_staff_terms_note().to_owned()),
    })
}

pub(crate) fn record_from_direct_asset_url(url: &str, base_url: &str) -> SourceRecord {
    record_from_direct_asset_link(
        url,
        "",
        &format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            OSD_JOINT_STAFF_READING_ROOM_PATH
        ),
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
        id: format!("{OSD_JOINT_STAFF_SOURCE}:{source_id}"),
        document_key: document_key(OSD_JOINT_STAFF_SOURCE, &source_id),
        source: OSD_JOINT_STAFF_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("OSD/Joint Staff FOIA Reading Room".to_owned()),
        record_group: Some("osd_joint_staff_foia_reading_room".to_owned()),
        description: Some(description_for_record(false).to_owned()),
        origin_url: listing_url.to_owned(),
        document_url: url.to_owned(),
        pdf_url: None,
        metadata,
        attachments: Vec::new(),
        text_preview: None,
        citation_note: Some(osd_joint_staff_citation_note().to_owned()),
        terms_note: Some(osd_joint_staff_terms_note().to_owned()),
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
        id: format!("{OSD_JOINT_STAFF_SOURCE}:{source_id}"),
        document_key: document_key(OSD_JOINT_STAFF_SOURCE, &source_id),
        source: OSD_JOINT_STAFF_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("OSD/Joint Staff FOIA Reading Room".to_owned()),
        record_group: Some("osd_joint_staff_foia_reading_room".to_owned()),
        description: Some(description_for_record(asset.role == SourceAssetRole::Pdf).to_owned()),
        origin_url: listing_url.to_owned(),
        document_url: url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(osd_joint_staff_citation_note().to_owned()),
        terms_note: Some(osd_joint_staff_terms_note().to_owned()),
    }
}

fn looks_like_reading_room_lead(url: &str, label: &str) -> bool {
    let lower_url = url.to_ascii_lowercase();
    let lower_label = label.to_ascii_lowercase();
    lower_url.contains("/records-declass/foia/reading-room/reading-room-list_2/")
        || lower_url.contains("/foid/reading-room/")
        || lower_url.contains("/portals/54/documents/foid/reading")
        || lower_url.ends_with(".pdf")
        || lower_label.contains("pdf")
        || lower_label.contains("joint staff")
        || lower_label.contains("reading room")
        || lower_label.contains("report")
        || lower_label.contains("strategy")
}

fn is_navigation_label(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "" | "home"
            | "foia"
            | "search"
            | "clear"
            | "contact us"
            | "privacy"
            | "foia.gov"
            | "usa.gov"
    )
}

fn base_metadata(url: &str, source_id: &str, title: &str) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata.insert("source_url".to_owned(), url.to_owned());
    metadata.insert("source_id".to_owned(), source_id.to_owned());
    metadata.insert("title".to_owned(), title.to_owned());
    metadata.insert("official_source".to_owned(), "www.esd.whs.mil".to_owned());
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

fn title_for_link(label: &str, source_id: &str) -> String {
    let label = label.trim();
    if !label.is_empty() {
        return label.to_owned();
    }
    title_from_source_id(source_id)
}

fn title_from_heading(heading: &str) -> String {
    heading
        .trim()
        .trim_start_matches("Washington Headquarters Services >")
        .trim_start_matches("Records/Declass >")
        .trim()
        .to_owned()
}

fn title_from_source_id(source_id: &str) -> String {
    source_id
        .split('/')
        .next_back()
        .unwrap_or(source_id)
        .split('?')
        .next()
        .unwrap_or(source_id)
        .replace(['_', '-'], " ")
        .trim()
        .to_owned()
}

fn description_for_record(has_pdf: bool) -> &'static str {
    if has_pdf {
        "Official OSD/Joint Staff FOIA Reading Room lead with PDF asset metadata."
    } else {
        "Official OSD/Joint Staff FOIA Reading Room lead; inspect linked assets before citation."
    }
}

fn preview(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect::<String>()
}

fn first_non_empty(values: &[Option<String>]) -> String {
    values
        .iter()
        .filter_map(|value| value.as_deref())
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned()
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
