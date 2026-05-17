use crate::sources::{SourceAssetRole, SourceRecord};

use super::asset::{asset_from_url, asset_priority_key, dedupe_assets, media_type_for_asset};
use super::classify::{
    base_metadata, category_from_url_and_title, description_for_category, enrich_metadata,
    is_asset_link, is_candidate_lead_link, title_for_listing_lead, title_from_source_id,
};
use super::html::{anchors, clean_html_text, first_tag_text};
use super::url::{absolutize, document_key, is_allowed_justice_epstein_url, slugify};
use super::{
    doj_epstein_citation_note, doj_epstein_terms_note, DOJ_EPSTEIN_DISCLOSURES_PATH,
    DOJ_EPSTEIN_DISCLOSURES_URL, DOJ_EPSTEIN_SOURCE,
};

const SENSITIVE_WARNING: &str = "DOJ Epstein Library materials may contain sensitive victim-identification or sexual-assault content; rely on DOJ redactions and report suspected disclosure issues to EFTA@usdoj.gov.";

pub(crate) fn records_from_disclosures_page(html: &str, base_url: &str) -> Vec<SourceRecord> {
    let mut records = Vec::new();

    for (href, text) in anchors(html) {
        let document_url = absolutize(&href, base_url);
        if !is_allowed_justice_epstein_url(&document_url, base_url) {
            continue;
        }
        if !is_candidate_lead_link(&document_url, &text) {
            continue;
        }

        let source_id = source_id_from_url(&document_url);
        let category = category_from_url_and_title(&document_url, &text);
        let title = title_for_listing_lead(&document_url, &text);
        let mut metadata = base_metadata(&document_url, category, SENSITIVE_WARNING);
        enrich_metadata(&mut metadata, category, &source_id, &title);

        records.push(SourceRecord {
            id: format!("{DOJ_EPSTEIN_SOURCE}:{source_id}"),
            document_key: document_key(DOJ_EPSTEIN_SOURCE, &source_id),
            source: DOJ_EPSTEIN_SOURCE,
            source_id,
            title,
            date: None,
            collection: Some("DOJ Epstein Library".to_owned()),
            record_group: Some(category.to_owned()),
            description: Some(description_for_category(category).to_owned()),
            origin_url: DOJ_EPSTEIN_DISCLOSURES_URL.to_owned(),
            document_url,
            pdf_url: None,
            metadata,
            attachments: Vec::new(),
            text_preview: None,
            citation_note: Some(doj_epstein_citation_note().to_owned()),
            terms_note: Some(doj_epstein_terms_note().to_owned()),
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

    let category = category_from_url_and_title(record_url, "");
    let heading = first_tag_text(html, "h1");
    let has_heading = !heading.trim().is_empty();
    let title = if has_heading {
        heading
    } else {
        title_from_source_id(&source_id)
    };

    let mut attachments = anchors(html)
        .into_iter()
        .filter_map(|(href, text)| {
            let asset_url = absolutize(&href, base_url);
            if !is_allowed_justice_epstein_url(&asset_url, base_url)
                || !is_asset_link(&asset_url, &text)
            {
                return None;
            }
            Some(asset_from_url(&asset_url))
        })
        .collect::<Vec<_>>();
    attachments.sort_by_key(asset_priority_key);
    attachments = dedupe_assets(attachments);

    let body_text = clean_html_text(html);
    if (!has_heading && attachments.is_empty()) || body_text.trim().is_empty() {
        return None;
    }

    let mut metadata = base_metadata(record_url, category, SENSITIVE_WARNING);
    enrich_metadata(&mut metadata, category, &source_id, &title);
    metadata.insert(
        "privacy_notice_present".to_owned(),
        body_text
            .to_ascii_lowercase()
            .contains("privacy notice")
            .to_string(),
    );
    metadata.insert(
        "age_verification_present".to_owned(),
        body_text
            .to_ascii_lowercase()
            .contains("are you 18 years of age or older")
            .to_string(),
    );

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    if let Some(first_asset) = attachments.first() {
        metadata.insert("asset_filename".to_owned(), first_asset.label.clone());
        metadata.insert(
            "media_type".to_owned(),
            media_type_for_asset(first_asset).to_owned(),
        );
    }

    Some(SourceRecord {
        id: format!("{DOJ_EPSTEIN_SOURCE}:{source_id}"),
        document_key: document_key(DOJ_EPSTEIN_SOURCE, &source_id),
        source: DOJ_EPSTEIN_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("DOJ Epstein Library".to_owned()),
        record_group: Some(category.to_owned()),
        description: Some(description_for_category(category).to_owned()),
        origin_url: DOJ_EPSTEIN_DISCLOSURES_URL.to_owned(),
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(doj_epstein_citation_note().to_owned()),
        terms_note: Some(doj_epstein_terms_note().to_owned()),
    })
}

pub(crate) fn detail_url_from_source_id(
    source_id: &str,
    epstein_home_url: &str,
    disclosures_url: &str,
) -> Option<String> {
    let source_id = source_id.trim();
    if source_id.is_empty() {
        return None;
    }
    if source_id == "library-home" {
        return Some(epstein_home_url.to_owned());
    }
    if source_id == "doj-disclosures" {
        return Some(disclosures_url.to_owned());
    }
    if let Some(media_id) = media_id_from_source_id(source_id) {
        let origin = epstein_home_url
            .trim_end_matches('/')
            .strip_suffix("/epstein")
            .unwrap_or_else(|| epstein_home_url.trim_end_matches('/'));
        return Some(format!(
            "{origin}/media/{}/dl",
            super::url::percent_encode_path_segment(media_id)
        ));
    }
    if source_id.contains('/') {
        return None;
    }

    Some(format!(
        "{disclosures_url}/{}",
        super::url::percent_encode_path_segment(source_id)
    ))
}

pub(crate) fn source_id_from_url(url: &str) -> String {
    let path = path_from_url(url);
    if path == "/epstein" || path == "/epstein/" {
        return "library-home".to_owned();
    }
    if path == DOJ_EPSTEIN_DISCLOSURES_PATH || path == format!("{DOJ_EPSTEIN_DISCLOSURES_PATH}/") {
        return "doj-disclosures".to_owned();
    }

    if let Some(tail) = path.split_once(&format!("{DOJ_EPSTEIN_DISCLOSURES_PATH}/")) {
        let slug = tail.1.split('/').next().unwrap_or("").trim();
        if !slug.is_empty() {
            return slug.to_owned();
        }
    }

    if let Some(media_tail) = path.split_once("/media/") {
        let slug = media_tail
            .1
            .trim_matches('/')
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .replace('/', "-");
        let slug = slugify(&slug);
        return if slug.is_empty() {
            "media-asset".to_owned()
        } else {
            format!("media-{slug}")
        };
    }

    let slug = slugify(&path);
    if slug.is_empty() {
        "epstein-record".to_owned()
    } else {
        slug
    }
}

pub(crate) fn sensitive_warning() -> &'static str {
    SENSITIVE_WARNING
}

fn media_id_from_source_id(source_id: &str) -> Option<&str> {
    let value = source_id.strip_prefix("media-")?;
    let media_id = value.strip_suffix("-dl").unwrap_or(value);
    (!media_id.is_empty()
        && media_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
    .then_some(media_id)
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

fn path_from_url(url: &str) -> String {
    let (_, rest) = url.split_once("://").unwrap_or(("", url));
    let path = rest.split_once('/').map(|(_, tail)| tail).unwrap_or("");
    let path = path.split('#').next().unwrap_or("");
    let path = path.split('?').next().unwrap_or("");
    format!("/{path}")
}
