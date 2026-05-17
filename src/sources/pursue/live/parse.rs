use std::collections::{BTreeMap, HashMap, HashSet};

use crate::sources::{SourceAssetRole, SourceRecord};

use super::asset::{asset_from_url, asset_priority_key, dedupe_assets, sanitize_id_component};
use super::csv::{normalize_field_name, parse_csv_rows, row_value};
use super::html::{anchors, first_non_empty, first_tag_text, release_date_from_index};
use super::url::{absolutize, document_key};
use super::{CITATION_NOTE, PURSUE_INDEX_PATH, PURSUE_SOURCE, TERMS_NOTE};

pub(crate) fn records_from_csv(csv_text: &str, base_url: &str) -> Vec<SourceRecord> {
    let rows = parse_csv_rows(csv_text);
    let Some((header, data_rows)) = rows.split_first() else {
        return Vec::new();
    };

    let mut keys = HashMap::new();
    for (index, field) in header.iter().enumerate() {
        keys.insert(normalize_field_name(field), index);
    }

    let mut records = Vec::new();
    for row in data_rows {
        let release_id = row_value(row, &keys, &["release", "release tranche", "tranche"])
            .as_deref()
            .map(normalize_release_id)
            .unwrap_or_else(|| "release-unknown".to_owned());
        let agency =
            row_value(row, &keys, &["agency"]).unwrap_or_else(|| "Unknown agency".to_owned());
        let incident_date = row_value(row, &keys, &["incident date"]);
        let release_date = row_value(row, &keys, &["release date"]);
        let incident_location = row_value(row, &keys, &["incident location"]);
        let document_type = row_value(row, &keys, &["type", "document type"]);
        let virin = row_value(row, &keys, &["virin"]);
        let file_name = row_value(row, &keys, &["file", "file name", "filename"]);
        let raw_url = row_value(row, &keys, &["url", "asset url", "link", "download"]);
        let release_page = row_value(row, &keys, &["release page", "release link", "details"]);

        let Some(asset_url_raw) = raw_url.or(file_name.clone()) else {
            continue;
        };
        let asset_url = absolutize(&asset_url_raw, base_url);
        if asset_url.trim().is_empty() {
            continue;
        }

        let source_id = make_record_source_id(&release_id, virin.as_deref(), file_name.as_deref());
        let asset = asset_from_url(&asset_url);
        let mut metadata = BTreeMap::new();
        metadata.insert("release_tranche".to_owned(), release_id.clone());
        metadata.insert("agency".to_owned(), agency.clone());
        if let Some(release_date) = &release_date {
            metadata.insert("release_date".to_owned(), release_date.clone());
        }
        if let Some(incident_date) = &incident_date {
            metadata.insert("incident_date".to_owned(), incident_date.clone());
        }
        if let Some(incident_location) = &incident_location {
            metadata.insert("incident_location".to_owned(), incident_location.clone());
        }
        if let Some(document_type) = &document_type {
            metadata.insert("document_type".to_owned(), document_type.clone());
        }
        if let Some(virin) = &virin {
            metadata.insert("virin".to_owned(), virin.clone());
        }
        if let Some(file_name) = &file_name {
            metadata.insert("asset_filename".to_owned(), file_name.clone());
        }

        let title = format!(
            "PURSUE {} {} {}",
            release_id.to_uppercase(),
            agency,
            document_type.as_deref().unwrap_or("record")
        )
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

        let description = Some(
            [
                incident_date.clone(),
                incident_location.clone(),
                document_type.clone(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" | "),
        )
        .filter(|value| !value.is_empty());

        let document_url = release_page
            .as_deref()
            .map(|url| absolutize(url, base_url))
            .unwrap_or_else(|| absolutize(PURSUE_INDEX_PATH, base_url));

        records.push(SourceRecord {
            id: format!("{PURSUE_SOURCE}:{source_id}"),
            document_key: document_key(PURSUE_SOURCE, &source_id),
            source: PURSUE_SOURCE,
            source_id,
            title,
            date: release_date,
            collection: Some("PURSUE".to_owned()),
            record_group: None,
            description,
            origin_url: absolutize(PURSUE_INDEX_PATH, base_url),
            document_url,
            pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
            metadata,
            attachments: vec![asset],
            text_preview: None,
            citation_note: Some(CITATION_NOTE.to_owned()),
            terms_note: Some(TERMS_NOTE.to_owned()),
        });
    }

    records
}

pub(crate) fn records_from_index_html(html: &str, base_url: &str) -> Vec<SourceRecord> {
    let mut records = Vec::new();
    let ufo_url = absolutize(PURSUE_INDEX_PATH, base_url);
    let release_anchors = anchors(html)
        .into_iter()
        .filter(|(href, text)| {
            let lower_href = href.to_ascii_lowercase();
            lower_href.contains("/news/releases/release/")
                || text.to_ascii_lowercase().contains("release")
        })
        .collect::<Vec<_>>();

    if release_anchors.is_empty() {
        return records;
    }

    let media_links = anchors(html)
        .into_iter()
        .filter_map(|(href, text)| {
            let asset_url = absolutize(&href, base_url);
            let lower_url = asset_url.to_ascii_lowercase();
            if !lower_url.contains("/medialink/ufo/")
                && !text.to_ascii_lowercase().contains("download")
            {
                return None;
            }
            Some(asset_from_url(&asset_url))
        })
        .collect::<Vec<_>>();

    for (href, text) in release_anchors {
        let release_url = absolutize(&href, base_url);
        let release_id =
            release_id_from_url(&release_url).unwrap_or_else(|| normalize_release_id(&text));
        let title = if text.trim().is_empty() {
            format!("PURSUE {}", release_id.to_uppercase())
        } else {
            text
        };
        let mut metadata = BTreeMap::new();
        metadata.insert("release_tranche".to_owned(), release_id.clone());

        let mut attachments = media_links.clone();
        attachments.sort_by_key(asset_priority_key);
        let pdf_url = attachments
            .iter()
            .find(|asset| asset.role == SourceAssetRole::Pdf)
            .map(|asset| asset.asset_url.clone());

        records.push(SourceRecord {
            id: format!("{PURSUE_SOURCE}:{release_id}"),
            document_key: document_key(PURSUE_SOURCE, &release_id),
            source: PURSUE_SOURCE,
            source_id: release_id,
            title,
            date: release_date_from_index(html),
            collection: Some("PURSUE".to_owned()),
            record_group: None,
            description: None,
            origin_url: ufo_url.clone(),
            document_url: release_url,
            pdf_url,
            metadata,
            attachments,
            text_preview: None,
            citation_note: Some(CITATION_NOTE.to_owned()),
            terms_note: Some(TERMS_NOTE.to_owned()),
        });
    }

    dedupe_records(records)
}

pub(crate) fn record_from_release_page(
    html: &str,
    base_url: &str,
    release_url: &str,
    release_id: &str,
) -> Option<SourceRecord> {
    let title = first_non_empty(&[
        first_tag_text(html, "h1"),
        first_tag_text(html, "title"),
        format!("PURSUE {}", release_id.to_uppercase()),
    ]);

    let mut metadata = BTreeMap::new();
    metadata.insert("release_tranche".to_owned(), release_id.to_owned());
    if let Some(release_date) = release_date_from_index(html) {
        metadata.insert("release_date".to_owned(), release_date);
    }

    let mut attachments = anchors(html)
        .into_iter()
        .filter_map(|(href, text)| {
            let asset_url = absolutize(&href, base_url);
            let lower_url = asset_url.to_ascii_lowercase();
            let lower_text = text.to_ascii_lowercase();
            if !lower_url.contains("/medialink/ufo/") && !lower_text.contains("download") {
                return None;
            }
            Some(asset_from_url(&asset_url))
        })
        .collect::<Vec<_>>();
    attachments.sort_by_key(asset_priority_key);
    attachments = dedupe_assets(attachments);

    if attachments.is_empty() {
        return None;
    }

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    Some(SourceRecord {
        id: format!("{PURSUE_SOURCE}:{release_id}"),
        document_key: document_key(PURSUE_SOURCE, release_id),
        source: PURSUE_SOURCE,
        source_id: release_id.to_owned(),
        title,
        date: metadata.get("release_date").cloned(),
        collection: Some("PURSUE".to_owned()),
        record_group: None,
        description: None,
        origin_url: absolutize(PURSUE_INDEX_PATH, base_url),
        document_url: release_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(CITATION_NOTE.to_owned()),
        terms_note: Some(TERMS_NOTE.to_owned()),
    })
}

pub(crate) fn record_matches_query(record: &SourceRecord, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {}",
        record.title,
        record.source_id,
        record.description.clone().unwrap_or_default(),
        record.date.clone().unwrap_or_default(),
        record.metadata.get("agency").cloned().unwrap_or_default(),
        record
            .metadata
            .get("incident_location")
            .cloned()
            .unwrap_or_default()
    )
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|term| haystack.contains(&term.to_ascii_lowercase()))
}

pub(crate) fn release_id_from_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if let Some((_, tail)) = lower.split_once("/ufo/releases/") {
        let id = tail.split(['/', '?', '#']).next()?.trim();
        if !id.is_empty() {
            return Some(id.to_owned());
        }
    }

    let release_token = lower
        .split(['/', '?', '#'])
        .find(|segment| segment.starts_with("release") && segment.len() <= 16)?;
    Some(normalize_release_id(release_token))
}

pub(crate) fn release_hint_from_asset_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let hint = lower
        .split('/')
        .find(|segment| segment.starts_with("release_") || segment.starts_with("release-"))?
        .replace('_', "-");
    Some(normalize_release_id(&hint))
}

pub(crate) fn release_id_from_article_html(html: &str) -> Option<String> {
    anchors(html)
        .into_iter()
        .map(|(href, _text)| absolutize(&href, "https://www.war.gov"))
        .find_map(|url| release_hint_from_asset_url(&url))
}

pub(crate) fn csv_links_from_html(html: &str, base_url: &str) -> Vec<String> {
    anchors(html)
        .into_iter()
        .map(|(href, _text)| href)
        .filter(|href| {
            let lower = href.to_ascii_lowercase();
            lower.contains("uap-release") && lower.ends_with(".csv")
        })
        .map(|href| absolutize(&href, base_url))
        .collect::<Vec<_>>()
}

pub(crate) fn dedupe_records(records: Vec<SourceRecord>) -> Vec<SourceRecord> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for record in records {
        if seen.insert(record.id.clone()) {
            deduped.push(record);
        }
    }
    deduped
}

pub(crate) fn normalize_release_id(value: &str) -> String {
    let lower = value.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return "release-unknown".to_owned();
    }

    if lower.starts_with("release-") {
        return lower;
    }

    if lower.starts_with("release ") {
        return format!("release-{}", lower.trim_start_matches("release ").trim());
    }

    lower.replace(' ', "-")
}

fn make_record_source_id(release_id: &str, virin: Option<&str>, file_name: Option<&str>) -> String {
    let suffix = virin
        .or(file_name)
        .map(sanitize_id_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "record".to_owned());
    format!("{}:{}", normalize_release_id(release_id), suffix)
}
