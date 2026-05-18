use scraper::{ElementRef, Html, Selector};

use crate::sources::{SourceAsset, SourceAssetRole, SourceError, SourceMetadata, SourceRecord};

use super::{doe_citation_note, doe_terms_note, DOE_SOURCE};
mod url;
pub(crate) use url::{detail_endpoint, parse_locator, search_endpoint, DoeLocator};
use url::{
    document_key, normalize_source_id, official_record_url, source_id_from_official_url,
    source_id_from_official_url_with_base,
};

const SOURCE_WARNING: &str = "DOE OpenNet records are official DOE/OSTI declassified-record leads; not every record has electronic full text, and page citations require PDF ingestion/page-boundary verification.";

pub(crate) fn records_from_search_html(
    body: &str,
    base_url: &str,
    origin_url: &str,
) -> Result<Vec<SourceRecord>, SourceError> {
    ensure_html_body(body, origin_url)?;
    let document = Html::parse_document(body);
    let table_selector = selector("#search-results-table")?;
    let Some(table) = document.select(&table_selector).next() else {
        return Err(SourceError::SourceChanged {
            source: DOE_SOURCE,
            message: "DOE OpenNet search response is missing the expected search results table."
                .to_owned(),
            url: Some(origin_url.to_owned()),
        });
    };

    let row_selector = selector("tbody > tr")?;
    let mut records = Vec::new();
    for row in table.select(&row_selector) {
        if let Some(record) = record_from_search_row(row, base_url, origin_url)? {
            records.push(record);
        }
    }
    Ok(dedupe_records(records))
}

pub(crate) fn record_from_detail_html(
    body: &str,
    base_url: &str,
    origin_url: &str,
    source_id_hint: Option<&str>,
) -> Result<SourceRecord, SourceError> {
    ensure_html_body(body, origin_url)?;
    let document = Html::parse_document(body);
    let detail_selector = selector("#detailTableContent")?;
    let Some(detail) = document.select(&detail_selector).next() else {
        return Err(SourceError::SourceChanged {
            source: DOE_SOURCE,
            message: "DOE OpenNet detail page is missing the expected detail metadata block."
                .to_owned(),
            url: Some(origin_url.to_owned()),
        });
    };

    let source_id = source_id_hint
        .and_then(normalize_source_id)
        .or_else(|| source_id_from_official_url(origin_url))
        .ok_or_else(|| SourceError::SourceChanged {
            source: DOE_SOURCE,
            message: "DOE OpenNet detail page is missing a stable OSTI id.".to_owned(),
            url: Some(origin_url.to_owned()),
        })?;
    let fields = detail_fields(&detail)?;
    let title = field_value(&fields, "Title").ok_or_else(|| SourceError::SourceChanged {
        source: DOE_SOURCE,
        message: "DOE OpenNet detail page is missing a title field.".to_owned(),
        url: Some(origin_url.to_owned()),
    })?;

    let document_url = official_record_url(&source_id);
    let mut attachments = extract_assets(&detail, base_url, &document_url)?;
    attachments.push(SourceAsset {
        asset_url: document_url.clone(),
        label: "DOE OpenNet Detail Page".to_owned(),
        mime_type: Some("text/html".to_owned()),
        role: SourceAssetRole::Html,
    });
    dedupe_assets(&mut attachments);
    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    let date = field_value(&fields, "Publication Date");
    let subject_terms = field_value(&fields, "Subject Terms");
    let document_location = field_value(&fields, "Document Location");
    let originating_org = field_value(&fields, "Originating Research Org.");

    let mut metadata = base_metadata(&source_id, &document_url);
    for (label, value) in fields {
        metadata.insert(metadata_key(&label), value);
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

    Ok(SourceRecord {
        id: format!("{DOE_SOURCE}:{source_id}"),
        document_key: document_key(&source_id),
        source: DOE_SOURCE,
        source_id,
        title,
        date,
        collection: Some("DOE OpenNet".to_owned()),
        record_group: originating_org.or(document_location),
        description: subject_terms,
        origin_url: origin_url.to_owned(),
        document_url,
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(doe_citation_note().to_owned()),
        terms_note: Some(doe_terms_note().to_owned()),
    })
}

fn record_from_search_row(
    row: ElementRef<'_>,
    base_url: &str,
    origin_url: &str,
) -> Result<Option<SourceRecord>, SourceError> {
    let anchor_selector = selector("td.hide-xs a[href*='detail?osti-id=']")?;
    let Some(anchor) = row.select(&anchor_selector).next() else {
        return Ok(None);
    };
    let title = clean_text(&anchor.text().collect::<Vec<_>>().join(" "));
    if title.is_empty() {
        return Ok(None);
    }
    let href = anchor.value().attr("href").unwrap_or_default();
    let absolute = absolutize(base_url, href);
    let Some(source_id) = source_id_from_official_url_with_base(&absolute, base_url) else {
        return Ok(None);
    };
    let document_url = official_record_url(&source_id);

    let cells = row
        .select(&selector("td.hide-xs")?)
        .map(|cell| clean_text(&cell.text().collect::<Vec<_>>().join(" ")))
        .collect::<Vec<_>>();
    let authors = cells.get(1).filter(|value| !value.is_empty()).cloned();
    let accession = cells.get(2).filter(|value| !value.is_empty()).cloned();
    let document_number = cells.get(3).filter(|value| !value.is_empty()).cloned();
    let document_type = cells.get(4).filter(|value| !value.is_empty()).cloned();
    let research_org = cells.get(5).filter(|value| !value.is_empty()).cloned();
    let entry_date = cells.get(6).filter(|value| !value.is_empty()).cloned();
    let publication_date = cells.get(7).filter(|value| !value.is_empty()).cloned();
    let declassification_date = cells.get(8).filter(|value| !value.is_empty()).cloned();

    let mut attachments = extract_assets(&row, base_url, &document_url)?;
    attachments.push(SourceAsset {
        asset_url: document_url.clone(),
        label: "DOE OpenNet Detail Page".to_owned(),
        mime_type: Some("text/html".to_owned()),
        role: SourceAssetRole::Html,
    });
    dedupe_assets(&mut attachments);
    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    let mut metadata = base_metadata(&source_id, &document_url);
    insert_optional(&mut metadata, "authors", authors.as_deref());
    insert_optional(&mut metadata, "accession_number", accession.as_deref());
    insert_optional(&mut metadata, "document_number", document_number.as_deref());
    insert_optional(&mut metadata, "document_type", document_type.as_deref());
    insert_optional(
        &mut metadata,
        "originating_research_org",
        research_org.as_deref(),
    );
    insert_optional(&mut metadata, "opennet_entry_date", entry_date.as_deref());
    insert_optional(
        &mut metadata,
        "declassification_date",
        declassification_date.as_deref(),
    );
    metadata.insert("listing_origin".to_owned(), "search".to_owned());
    metadata.insert("asset_count".to_owned(), attachments.len().to_string());
    metadata.insert(
        "pdf_asset_count".to_owned(),
        attachments
            .iter()
            .filter(|asset| asset.role == SourceAssetRole::Pdf)
            .count()
            .to_string(),
    );

    Ok(Some(SourceRecord {
        id: format!("{DOE_SOURCE}:{source_id}"),
        document_key: document_key(&source_id),
        source: DOE_SOURCE,
        source_id,
        title,
        date: publication_date,
        collection: Some("DOE OpenNet".to_owned()),
        record_group: research_org,
        description: document_type,
        origin_url: origin_url.to_owned(),
        document_url,
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(doe_citation_note().to_owned()),
        terms_note: Some(doe_terms_note().to_owned()),
    }))
}

fn detail_fields(detail: &ElementRef<'_>) -> Result<Vec<(String, String)>, SourceError> {
    let row_selector = selector(".row")?;
    let label_selector = selector(".detailsTH")?;
    let value_selector = selector(".detailsTHData")?;
    let mut fields = Vec::new();
    for row in detail.select(&row_selector) {
        let Some(label) = row.select(&label_selector).next() else {
            continue;
        };
        let Some(value) = row.select(&value_selector).next() else {
            continue;
        };
        let label = clean_text(&label.text().collect::<Vec<_>>().join(" "))
            .trim_end_matches(':')
            .to_owned();
        let value = clean_text(&value.text().collect::<Vec<_>>().join(" "));
        if !label.is_empty() && !value.is_empty() {
            fields.push((label, value));
        }
    }
    Ok(fields)
}

fn field_value(fields: &[(String, String)], label: &str) -> Option<String> {
    fields
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(label))
        .map(|(_, value)| value.clone())
}

fn extract_assets(
    scope: &ElementRef<'_>,
    base_url: &str,
    document_url: &str,
) -> Result<Vec<SourceAsset>, SourceError> {
    let link_selector = selector("a[href]")?;
    let mut assets = Vec::new();
    for anchor in scope.select(&link_selector) {
        let href = anchor.value().attr("href").unwrap_or_default().trim();
        if href.is_empty() || href.starts_with("mailto:") {
            continue;
        }
        let absolute = absolutize(base_url, href);
        if !is_official_osti_asset_url(&absolute) {
            continue;
        }
        let label = clean_text(&anchor.text().collect::<Vec<_>>().join(" "));
        let lower = absolute.to_ascii_lowercase();
        let (role, mime_type, default_label) = if lower.ends_with(".pdf") {
            (
                SourceAssetRole::Pdf,
                Some("application/pdf".to_owned()),
                "DOE OpenNet PDF",
            )
        } else if lower.contains("/opennet/detail") || lower.ends_with(".html") {
            (
                SourceAssetRole::Html,
                Some("text/html".to_owned()),
                "DOE OpenNet Page",
            )
        } else {
            (SourceAssetRole::Other, None, "DOE OpenNet Asset")
        };
        if absolute == document_url && role == SourceAssetRole::Html {
            continue;
        }
        assets.push(SourceAsset {
            asset_url: absolute,
            label: if label.is_empty() {
                default_label.to_owned()
            } else {
                label
            },
            mime_type,
            role,
        });
    }
    Ok(assets)
}

fn is_official_osti_asset_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://www.osti.gov/opennet/")
        || lower.starts_with("http://www.osti.gov/opennet/")
}

fn base_metadata(source_id: &str, document_url: &str) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("osti_id".to_owned(), source_id.to_owned());
    metadata.insert("official_opennet_url".to_owned(), document_url.to_owned());
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata
}

fn insert_optional(metadata: &mut SourceMetadata, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        metadata.insert(key.to_owned(), value.to_owned());
    }
}

fn metadata_key(label: &str) -> String {
    label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn dedupe_records(records: Vec<SourceRecord>) -> Vec<SourceRecord> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::new();
    for record in records {
        if seen.insert(record.id.clone()) {
            deduped.push(record);
        }
    }
    deduped
}

fn dedupe_assets(assets: &mut Vec<SourceAsset>) {
    assets.sort_by(|left, right| {
        asset_rank(left)
            .cmp(&asset_rank(right))
            .then_with(|| left.asset_url.cmp(&right.asset_url))
    });
    assets.dedup_by(|left, right| left.asset_url == right.asset_url);
}

fn asset_rank(asset: &SourceAsset) -> u8 {
    match asset.role {
        SourceAssetRole::Pdf => 0,
        SourceAssetRole::Html => 1,
        _ => 2,
    }
}

fn absolutize(base_url: &str, href: &str) -> String {
    url::absolutize(base_url, href)
}

fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ensure_html_body(body: &str, origin_url: &str) -> Result<(), SourceError> {
    let lower = body.to_ascii_lowercase();
    if lower.contains("<html") && lower.contains("</html>") {
        Ok(())
    } else {
        Err(SourceError::SourceChanged {
            source: DOE_SOURCE,
            message: "DOE OpenNet response was not an HTML document.".to_owned(),
            url: Some(origin_url.to_owned()),
        })
    }
}

fn selector(pattern: &str) -> Result<Selector, SourceError> {
    Selector::parse(pattern).map_err(|_| SourceError::SourceChanged {
        source: DOE_SOURCE,
        message: format!("Internal DOE OpenNet selector failed to parse: {pattern}"),
        url: None,
    })
}
