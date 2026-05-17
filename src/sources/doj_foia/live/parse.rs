use crate::sources::SourceRecord;

use super::asset::{asset_from_url, asset_priority_key, dedupe_assets, media_type_for_asset};
use super::classify::{
    asset_is_pdf, description_for_category, disclosure_category_for_url, is_candidate_asset_link,
    is_candidate_component_link, looks_like_component_name,
};
use super::html::{anchors, first_non_empty, first_tag_text};
use super::url::{
    absolutize, canonicalize_official_url, document_key, is_allowed_component_url,
    source_id_from_component_name, source_id_from_url,
};
use super::{doj_foia_citation_note, doj_foia_terms_note, DOJ_FOIA_SOURCE};

pub(crate) fn records_from_index_page(html: &str, index_url: &str) -> Vec<SourceRecord> {
    let mut records = Vec::new();

    for (href, link_text) in anchors(html) {
        let component_name = link_text.trim();
        let document_url = canonicalize_official_url(&absolutize(&href, index_url));
        if !is_allowed_component_url(&document_url, index_url)
            || !is_candidate_component_link(&document_url, component_name)
        {
            continue;
        }

        let source_id = source_id_from_component_name(component_name);
        let category = disclosure_category_for_url(&document_url);
        let mut metadata = base_metadata(index_url, &document_url, component_name, category);
        metadata.insert(
            "lead_origin".to_owned(),
            "oip_all_components_index".to_owned(),
        );

        records.push(SourceRecord {
            id: format!("{DOJ_FOIA_SOURCE}:{source_id}"),
            document_key: document_key(DOJ_FOIA_SOURCE, &source_id),
            source: DOJ_FOIA_SOURCE,
            source_id,
            title: format!("{component_name} FOIA/Disclosure Library"),
            date: None,
            collection: Some("DOJ Component FOIA/Disclosure Libraries".to_owned()),
            record_group: Some(category.to_owned()),
            description: Some(description_for_category(category).to_owned()),
            origin_url: index_url.to_owned(),
            document_url,
            pdf_url: None,
            metadata,
            attachments: Vec::new(),
            text_preview: None,
            citation_note: Some(doj_foia_citation_note().to_owned()),
            terms_note: Some(doj_foia_terms_note().to_owned()),
        });
    }

    dedupe_records(records)
}

pub(crate) fn record_from_component_page(
    html: &str,
    index_url: &str,
    record_url: &str,
    component_hint: Option<&str>,
) -> Option<SourceRecord> {
    let component_hint = component_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let content = primary_content_html(html);
    let heading = first_non_empty(&[
        first_tag_text(content, "h1"),
        first_tag_text(content, "h2"),
        first_tag_text(content, "title"),
    ]);

    let component_name = component_hint
        .filter(|value| looks_like_component_name(value))
        .or_else(|| component_from_heading(&heading))
        .unwrap_or_else(|| "DOJ Component".to_owned());
    let source_id = source_id_from_component_name(&component_name);
    let category = disclosure_category_for_url(record_url);

    let mut attachments = anchors(content)
        .into_iter()
        .filter_map(|(href, link_text)| {
            let asset_url = canonicalize_official_url(&absolutize(&href, record_url));
            if !is_allowed_component_url(&asset_url, index_url)
                || !is_candidate_asset_link(&asset_url, &link_text)
            {
                return None;
            }
            Some(asset_from_url(&asset_url))
        })
        .collect::<Vec<_>>();
    attachments.sort_by_key(asset_priority_key);
    attachments = dedupe_assets(attachments);

    if heading.trim().is_empty() && attachments.is_empty() {
        return None;
    }

    let pdf_url = attachments
        .iter()
        .find(|asset| asset_is_pdf(asset))
        .map(|asset| asset.asset_url.clone());

    let mut metadata = base_metadata(index_url, record_url, &component_name, category);
    metadata.insert("lead_origin".to_owned(), "component_page".to_owned());
    metadata.insert(
        "page_heading".to_owned(),
        if heading.trim().is_empty() {
            component_name.clone()
        } else {
            heading.clone()
        },
    );
    metadata.insert("asset_count".to_owned(), attachments.len().to_string());
    metadata.insert(
        "pdf_asset_count".to_owned(),
        attachments
            .iter()
            .filter(|asset| asset_is_pdf(asset))
            .count()
            .to_string(),
    );
    if let Some(first_asset) = attachments.first() {
        metadata.insert("asset_filename".to_owned(), first_asset.label.clone());
        metadata.insert(
            "media_type".to_owned(),
            media_type_for_asset(first_asset).to_owned(),
        );
    }

    let title = if heading.trim().is_empty() {
        format!("{component_name} FOIA/Disclosure Library")
    } else {
        heading
    };

    Some(SourceRecord {
        id: format!("{DOJ_FOIA_SOURCE}:{source_id}"),
        document_key: document_key(DOJ_FOIA_SOURCE, &source_id),
        source: DOJ_FOIA_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some("DOJ Component FOIA/Disclosure Libraries".to_owned()),
        record_group: Some(category.to_owned()),
        description: Some(description_for_category(category).to_owned()),
        origin_url: index_url.to_owned(),
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(doj_foia_citation_note().to_owned()),
        terms_note: Some(doj_foia_terms_note().to_owned()),
    })
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
        record
            .metadata
            .get("component_name")
            .map(String::as_str)
            .unwrap_or_default(),
        record.record_group.as_deref().unwrap_or_default(),
    )
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

pub(crate) fn single_asset_record(
    asset_url: &str,
    index_url: &str,
    component_hint: Option<&str>,
) -> SourceRecord {
    let asset = asset_from_url(asset_url);
    let component_name = component_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("DOJ Component")
        .to_owned();
    let source_id = source_id_from_url(asset_url);
    let category = disclosure_category_for_url(asset_url);

    let mut metadata = base_metadata(index_url, asset_url, &component_name, category);
    metadata.insert("lead_origin".to_owned(), "direct_asset_url".to_owned());
    metadata.insert("asset_filename".to_owned(), asset.label.clone());
    metadata.insert(
        "media_type".to_owned(),
        media_type_for_asset(&asset).to_owned(),
    );

    SourceRecord {
        id: format!("{DOJ_FOIA_SOURCE}:{source_id}"),
        document_key: document_key(DOJ_FOIA_SOURCE, &source_id),
        source: DOJ_FOIA_SOURCE,
        source_id,
        title: format!("{component_name} disclosure asset {}", asset.label),
        date: None,
        collection: Some("DOJ Component FOIA/Disclosure Libraries".to_owned()),
        record_group: Some(category.to_owned()),
        description: Some(description_for_category(category).to_owned()),
        origin_url: index_url.to_owned(),
        document_url: asset_url.to_owned(),
        pdf_url: asset_is_pdf(&asset).then(|| asset.asset_url.clone()),
        metadata,
        attachments: vec![asset],
        text_preview: None,
        citation_note: Some(doj_foia_citation_note().to_owned()),
        terms_note: Some(doj_foia_terms_note().to_owned()),
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

fn component_from_heading(heading: &str) -> Option<String> {
    let trimmed = heading.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed
        .replace("FOIA Library", "")
        .replace("FOIA Reading Room Records", "")
        .replace("FOIA Electronic Reading Room", "")
        .replace("FOIA", "")
        .trim()
        .to_owned();
    if looks_like_component_name(&normalized) {
        return Some(normalized);
    }

    looks_like_component_name(trimmed).then(|| trimmed.to_owned())
}

fn primary_content_html(html: &str) -> &str {
    if let Some(start) = html.find("<article") {
        let tail = &html[start..];
        if let Some(end) = tail.find("</article>") {
            return &tail[..end];
        }
        return tail;
    }
    if let Some(start) = html.find("<main") {
        let tail = &html[start..];
        if let Some(end) = tail.find("</main>") {
            return &tail[..end];
        }
        return tail;
    }
    if let Some(start) = html.find("field_body") {
        let tail = &html[start..];
        if let Some(end) = tail.find("</div>") {
            return &tail[..end];
        }
        return tail;
    }
    html
}

fn base_metadata(
    index_url: &str,
    official_url: &str,
    component_name: &str,
    category: &str,
) -> crate::sources::SourceMetadata {
    let mut metadata = crate::sources::SourceMetadata::new();
    metadata.insert("component_name".to_owned(), component_name.to_owned());
    metadata.insert(
        "component_slug".to_owned(),
        source_id_from_component_name(component_name),
    );
    metadata.insert("disclosure_category".to_owned(), category.to_owned());
    metadata.insert(
        "foia_provenance".to_owned(),
        "doj_component_proactive_disclosure_index".to_owned(),
    );
    metadata.insert("source_index_url".to_owned(), index_url.to_owned());
    metadata.insert("official_url".to_owned(), official_url.to_owned());
    metadata.insert(
        "linked_host".to_owned(),
        linked_host(official_url).unwrap_or_else(|| "unknown".to_owned()),
    );
    metadata.insert(
        "source_warning".to_owned(),
        "Use official DOJ component FOIA/disclosure pages and verify publication context/date before citation.".to_owned(),
    );
    metadata
}

fn linked_host(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?.to_ascii_lowercase();
    Some(host)
}
