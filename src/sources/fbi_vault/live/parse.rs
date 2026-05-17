use crate::sources::{SourceAssetRole, SourceMetadata, SourceRecord};

use super::asset::{asset_from_link, asset_priority_key, dedupe_assets, part_number_from_text};
use super::classify::{
    category_for_url, collection_name_from_source_id, description_for_category,
    is_candidate_asset_link, is_candidate_record_link, title_for_search_result,
};
use super::html::{anchors, clean_html_text, first_non_empty, first_tag_text};
use super::url::{absolutize, document_key, is_allowed_vault_url, source_id_from_url};
use super::{fbi_vault_citation_note, fbi_vault_terms_note, FBI_VAULT_SOURCE};

const SOURCE_WARNING: &str = "FBI Vault files can be multipart and historically uneven; preserve official page/PDF provenance and verify part ordering/page boundaries before citation.";

pub(crate) fn records_from_search_page(html: &str, base_url: &str) -> Vec<SourceRecord> {
    let mut records = Vec::new();

    for (href, text) in anchors(html) {
        let document_url = absolutize(&href, base_url);
        if !is_allowed_vault_url(&document_url, base_url) {
            continue;
        }
        if !is_candidate_record_link(&document_url, &text) {
            continue;
        }

        let source_id = source_id_from_url(&document_url);
        let title = title_for_search_result(&text, &source_id);
        let category = category_for_url(&document_url);
        let collection_name = collection_name_from_source_id(&source_id);
        let part_number = part_number_from_text(&title);

        let mut metadata = base_metadata(
            &document_url,
            &source_id,
            &title,
            &collection_name,
            category,
        );
        metadata.insert("listing_origin".to_owned(), "search".to_owned());
        if let Some(part_number) = part_number {
            metadata.insert("part_number".to_owned(), part_number.to_string());
            metadata.insert("part_label".to_owned(), title.clone());
            metadata.insert("part_sort_key".to_owned(), format!("{part_number:05}"));
        }

        records.push(SourceRecord {
            id: format!("{FBI_VAULT_SOURCE}:{source_id}"),
            document_key: document_key(FBI_VAULT_SOURCE, &source_id),
            source: FBI_VAULT_SOURCE,
            source_id,
            title,
            date: None,
            collection: Some(collection_name),
            record_group: Some(category.to_owned()),
            description: Some(description_for_category(category).to_owned()),
            origin_url: format!("{}/search", base_url.trim_end_matches('/')),
            document_url,
            pdf_url: None,
            metadata,
            attachments: Vec::new(),
            text_preview: None,
            citation_note: Some(fbi_vault_citation_note().to_owned()),
            terms_note: Some(fbi_vault_terms_note().to_owned()),
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

    let mut attachments = anchors(html)
        .into_iter()
        .filter_map(|(href, label)| {
            let asset_url = absolutize(&href, record_url);
            if !is_allowed_vault_url(&asset_url, base_url) {
                return None;
            }
            if !is_candidate_asset_link(&asset_url, &label) {
                return None;
            }
            Some(asset_from_link(&asset_url, &label))
        })
        .collect::<Vec<_>>();
    attachments.sort_by_key(asset_priority_key);
    attachments = dedupe_assets(attachments);

    let body_text = clean_html_text(html);
    if heading.trim().is_empty() && attachments.is_empty() && body_text.trim().is_empty() {
        return None;
    }

    let title = if heading.trim().is_empty() {
        title_for_search_result("", &source_id)
    } else {
        heading
    };
    if attachments.is_empty() {
        let heading_lower = title.to_ascii_lowercase();
        let body_lower = body_text.to_ascii_lowercase();
        let looks_like_vault_file_page = heading_lower.contains("vault")
            || heading_lower.contains("part")
            || body_lower.contains("download pdf")
            || body_lower.contains("at_download/file");
        if !looks_like_vault_file_page {
            return None;
        }
    }
    let category = category_for_url(record_url);
    let collection_name = collection_name_from_source_id(&source_id);
    let mut metadata = base_metadata(record_url, &source_id, &title, &collection_name, category);
    metadata.insert("listing_origin".to_owned(), "vault_record_page".to_owned());
    metadata.insert("asset_count".to_owned(), attachments.len().to_string());

    let pdf_count = attachments
        .iter()
        .filter(|asset| asset.role == SourceAssetRole::Pdf)
        .count();
    metadata.insert("pdf_asset_count".to_owned(), pdf_count.to_string());

    if let Some(primary) = attachments.first() {
        metadata.insert("primary_asset_label".to_owned(), primary.label.clone());
    }
    if !attachments.is_empty() {
        let labels = attachments
            .iter()
            .map(|asset| asset.label.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        metadata.insert("asset_labels".to_owned(), labels);
    }

    let part_count = attachments
        .iter()
        .filter_map(|asset| part_number_from_text(&asset.label))
        .count();
    if part_count > 0 {
        metadata.insert("part_count".to_owned(), part_count.to_string());
    }

    if let Some(part_number) = part_number_from_text(&title).or_else(|| {
        attachments
            .first()
            .and_then(|asset| part_number_from_text(&asset.label))
    }) {
        metadata.insert("part_number".to_owned(), part_number.to_string());
        metadata.insert("part_label".to_owned(), title.clone());
        metadata.insert("part_sort_key".to_owned(), format!("{part_number:05}"));
    }

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    Some(SourceRecord {
        id: format!("{FBI_VAULT_SOURCE}:{source_id}"),
        document_key: document_key(FBI_VAULT_SOURCE, &source_id),
        source: FBI_VAULT_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some(collection_name),
        record_group: Some(category.to_owned()),
        description: Some(description_for_category(category).to_owned()),
        origin_url: format!("{}/", base_url.trim_end_matches('/')),
        document_url: record_url.to_owned(),
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(fbi_vault_citation_note().to_owned()),
        terms_note: Some(fbi_vault_terms_note().to_owned()),
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
        record.collection.as_deref().unwrap_or_default(),
        record.record_group.as_deref().unwrap_or_default(),
    )
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

pub(crate) fn single_asset_record(asset_url: &str, base_url: &str) -> SourceRecord {
    let source_id = source_id_from_url(asset_url);
    let title = title_for_search_result("", &source_id);
    let collection_name = collection_name_from_source_id(&source_id);
    let mut asset = asset_from_link(asset_url, "Download PDF");
    if asset.role != SourceAssetRole::Pdf && asset.asset_url.ends_with("/at_download/file") {
        asset.role = SourceAssetRole::Pdf;
        asset.mime_type = Some("application/pdf".to_owned());
    }

    let mut metadata = base_metadata(
        asset_url,
        &source_id,
        &title,
        &collection_name,
        category_for_url(asset_url),
    );
    metadata.insert("listing_origin".to_owned(), "direct_asset_url".to_owned());
    metadata.insert("asset_count".to_owned(), "1".to_owned());
    metadata.insert(
        "pdf_asset_count".to_owned(),
        if asset.role == SourceAssetRole::Pdf {
            "1".to_owned()
        } else {
            "0".to_owned()
        },
    );
    metadata.insert("primary_asset_label".to_owned(), asset.label.clone());
    metadata.insert("asset_labels".to_owned(), asset.label.clone());
    if let Some(part_number) = part_number_from_text(&asset.label) {
        metadata.insert("part_number".to_owned(), part_number.to_string());
        metadata.insert("part_label".to_owned(), asset.label.clone());
        metadata.insert("part_sort_key".to_owned(), format!("{part_number:05}"));
    }

    SourceRecord {
        id: format!("{FBI_VAULT_SOURCE}:{source_id}"),
        document_key: document_key(FBI_VAULT_SOURCE, &source_id),
        source: FBI_VAULT_SOURCE,
        source_id,
        title,
        date: None,
        collection: Some(collection_name),
        record_group: Some(category_for_url(asset_url).to_owned()),
        description: Some(description_for_category(category_for_url(asset_url)).to_owned()),
        origin_url: format!("{}/", base_url.trim_end_matches('/')),
        document_url: asset_url.to_owned(),
        pdf_url: (asset.role == SourceAssetRole::Pdf).then(|| asset.asset_url.clone()),
        metadata,
        attachments: vec![asset],
        text_preview: None,
        citation_note: Some(fbi_vault_citation_note().to_owned()),
        terms_note: Some(fbi_vault_terms_note().to_owned()),
    }
}

fn base_metadata(
    official_url: &str,
    source_id: &str,
    title: &str,
    collection_name: &str,
    category: &str,
) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("official_page_url".to_owned(), official_url.to_owned());
    metadata.insert("vault_slug".to_owned(), source_id.to_owned());
    metadata.insert("vault_path".to_owned(), format!("/{source_id}"));
    metadata.insert("collection_name".to_owned(), collection_name.to_owned());
    metadata.insert("file_title".to_owned(), title.to_owned());
    metadata.insert("record_category".to_owned(), category.to_owned());
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata
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
