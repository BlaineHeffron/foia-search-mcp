use scraper::{ElementRef, Html};

use crate::sources::{SourceAsset, SourceAssetRole, SourceError, SourceMetadata, SourceRecord};

use super::assets::{dedupe_and_sort_assets, dedupe_search_records};
use super::html::{clean_text, ensure_html_body, first_text, selector};
use super::url::{absolutize, document_key, normalize_source_id, official_record_url};
use super::{noaa_citation_note, noaa_terms_note, NOAA_SOURCE};

const SOURCE_WARNING: &str = "NOAA repository records can mix government/public-domain and third-party rights statements; preserve the official repository URL, DOI/handle, and item-level terms before citation.";

pub(crate) fn records_from_search_html(
    body: &str,
    base_url: &str,
    origin_url: &str,
) -> Result<Vec<SourceRecord>, SourceError> {
    ensure_html_body(body, origin_url)?;

    let document = Html::parse_document(body);
    let container_selector = selector("#search-results, .search-results, main")?;
    let Some(container) = document.select(&container_selector).next() else {
        return Err(SourceError::SourceChanged {
            source: NOAA_SOURCE,
            message:
                "NOAA search response is missing the expected search-results container markup."
                    .to_owned(),
            url: Some(origin_url.to_owned()),
        });
    };

    let item_selector = selector(
        "article.ir-search-result, article.search-result, li.ir-search-result, li.search-result",
    )?;
    let mut records = Vec::new();
    for item in container.select(&item_selector) {
        if let Some(record) = record_from_search_item(item, base_url, origin_url)? {
            records.push(record);
        }
    }

    if records.is_empty() {
        let text = clean_text(&container.text().collect::<Vec<_>>().join(" ")).to_ascii_lowercase();
        if text.contains("no results")
            || text.contains("no matching")
            || text.contains("0 results")
            || text.contains("search results")
        {
            return Ok(Vec::new());
        }
    }

    Ok(dedupe_search_records(records))
}

pub(crate) fn record_from_detail_html(
    body: &str,
    base_url: &str,
    origin_url: &str,
    source_id_hint: Option<&str>,
) -> Result<SourceRecord, SourceError> {
    ensure_html_body(body, origin_url)?;

    let document = Html::parse_document(body);
    let detail_selector = selector("#record-detail, .item-summary-view, article.ir-record")?;
    let Some(detail) = document.select(&detail_selector).next() else {
        return Err(SourceError::SourceChanged {
            source: NOAA_SOURCE,
            message: "NOAA detail page format may have changed; expected detail metadata block was not found."
                .to_owned(),
            url: Some(origin_url.to_owned()),
        });
    };

    let source_id = source_id_hint
        .and_then(normalize_source_id)
        .or_else(|| detail.value().attr("data-id").and_then(normalize_source_id))
        .or_else(|| detail.value().attr("data-pid").and_then(normalize_source_id))
        .or_else(|| normalize_source_id(origin_url))
        .ok_or_else(|| SourceError::SourceChanged {
            source: NOAA_SOURCE,
            message: "NOAA detail metadata is missing a stable repository identifier (expected /view/noaa/<id>)."
                .to_owned(),
            url: Some(origin_url.to_owned()),
        })?;

    let title = first_text(&detail, &[".item-title", "h1", "h2", "title"]).ok_or_else(|| {
        SourceError::SourceChanged {
            source: NOAA_SOURCE,
            message: "NOAA detail page is missing a title field.".to_owned(),
            url: Some(origin_url.to_owned()),
        }
    })?;

    let description = first_text(&detail, &[".abstract", ".description", ".summary"]);
    let date = first_text(&detail, &[".date", ".issued", ".pub-date"]);
    let collection = first_text(&detail, &[".collection", ".line-office"]);
    let office = first_text(&detail, &[".office", ".program", ".localcorpname"]);
    let report_number = first_text(&detail, &[".report-number", ".series-number", ".report_no"]);
    let doi = first_text(&detail, &[".doi", ".identifier-doi"]);
    let handle = first_text(&detail, &[".handle", ".identifier-handle"]);
    let rights = first_text(&detail, &[".rights", ".usage-rights"]);
    let license = first_text(&detail, &[".license"]);
    let keywords = first_text(&detail, &[".keywords", ".subjects"]);
    let authors = first_text(&detail, &[".authors", ".creator", ".author"]);

    let document_url = official_record_url(&source_id);
    let mut attachments = extract_assets(&detail, base_url, &source_id, &document_url)?;
    attachments.push(SourceAsset {
        asset_url: document_url.clone(),
        label: "Repository Landing Page".to_owned(),
        mime_type: Some("text/html".to_owned()),
        role: SourceAssetRole::Html,
    });
    attachments = dedupe_and_sort_assets(attachments);

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    let mut metadata = base_metadata(&source_id, &document_url);
    if let Some(authors) = authors.as_deref() {
        metadata.insert("authors".to_owned(), authors.to_owned());
    }
    if let Some(collection) = collection.as_deref() {
        metadata.insert("collection_name".to_owned(), collection.to_owned());
    }
    if let Some(office) = office.as_deref() {
        metadata.insert("noaa_office_program".to_owned(), office.to_owned());
    }
    if let Some(value) = report_number.as_deref() {
        metadata.insert("report_number".to_owned(), value.to_owned());
    }
    if let Some(value) = doi.as_deref() {
        metadata.insert("doi".to_owned(), value.to_owned());
    }
    if let Some(value) = handle.as_deref() {
        metadata.insert("handle".to_owned(), value.to_owned());
    }
    if let Some(value) = rights.as_deref() {
        metadata.insert("rights".to_owned(), value.to_owned());
    }
    if let Some(value) = license.as_deref() {
        metadata.insert("license".to_owned(), value.to_owned());
    }
    if let Some(value) = keywords.as_deref() {
        metadata.insert("keywords".to_owned(), value.to_owned());
    }
    if let Some(value) = description.as_deref() {
        metadata.insert("abstract".to_owned(), value.to_owned());
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
        id: format!("{NOAA_SOURCE}:{source_id}"),
        document_key: document_key(&source_id),
        source: NOAA_SOURCE,
        source_id,
        title,
        date,
        collection,
        record_group: office,
        description,
        origin_url: origin_url.to_owned(),
        document_url,
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(noaa_citation_note().to_owned()),
        terms_note: Some(noaa_terms_note().to_owned()),
    })
}

fn record_from_search_item(
    item: ElementRef<'_>,
    base_url: &str,
    origin_url: &str,
) -> Result<Option<SourceRecord>, SourceError> {
    let anchor_selector = selector("h3 a, h2 a, a.title-link, a.item-title")?;
    let Some(anchor) = item.select(&anchor_selector).next() else {
        return Ok(None);
    };

    let title = clean_text(&anchor.text().collect::<Vec<_>>().join(" "));
    if title.is_empty() {
        return Ok(None);
    }

    let href = anchor.value().attr("href").unwrap_or_default();
    let source_id = item
        .value()
        .attr("data-id")
        .and_then(normalize_source_id)
        .or_else(|| item.value().attr("data-pid").and_then(normalize_source_id))
        .or_else(|| normalize_source_id(href))
        .or_else(|| {
            let absolute = absolutize(base_url, href);
            normalize_source_id(&absolute)
        });

    let Some(source_id) = source_id else {
        return Ok(None);
    };

    let document_url = official_record_url(&source_id);
    let description = first_text(&item, &[".description", ".abstract", ".summary"]);
    let date = first_text(&item, &[".date", ".issued", ".pub-date"]);
    let collection = first_text(&item, &[".collection", ".line-office"]);
    let office = first_text(&item, &[".office", ".program", ".localcorpname"]);
    let authors = first_text(&item, &[".authors", ".creator", ".author"]);
    let report_number = first_text(&item, &[".report-number", ".series-number", ".report_no"]);
    let doi = first_text(&item, &[".doi", ".identifier-doi"]);

    let mut attachments = extract_assets(&item, base_url, &source_id, &document_url)?;
    attachments.push(SourceAsset {
        asset_url: document_url.clone(),
        label: "Repository Landing Page".to_owned(),
        mime_type: Some("text/html".to_owned()),
        role: SourceAssetRole::Html,
    });
    attachments = dedupe_and_sort_assets(attachments);

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    let mut metadata = base_metadata(&source_id, &document_url);
    metadata.insert("listing_origin".to_owned(), "search".to_owned());
    if let Some(authors) = authors.as_deref() {
        metadata.insert("authors".to_owned(), authors.to_owned());
    }
    if let Some(collection) = collection.as_deref() {
        metadata.insert("collection_name".to_owned(), collection.to_owned());
    }
    if let Some(office) = office.as_deref() {
        metadata.insert("noaa_office_program".to_owned(), office.to_owned());
    }
    if let Some(value) = report_number.as_deref() {
        metadata.insert("report_number".to_owned(), value.to_owned());
    }
    if let Some(value) = doi.as_deref() {
        metadata.insert("doi".to_owned(), value.to_owned());
    }
    if let Some(value) = description.as_deref() {
        metadata.insert("abstract".to_owned(), value.to_owned());
    }

    Ok(Some(SourceRecord {
        id: format!("{NOAA_SOURCE}:{source_id}"),
        document_key: document_key(&source_id),
        source: NOAA_SOURCE,
        source_id,
        title,
        date,
        collection,
        record_group: office,
        description,
        origin_url: origin_url.to_owned(),
        document_url,
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(noaa_citation_note().to_owned()),
        terms_note: Some(noaa_terms_note().to_owned()),
    }))
}

fn extract_assets(
    scope: &ElementRef<'_>,
    base_url: &str,
    source_id: &str,
    document_url: &str,
) -> Result<Vec<SourceAsset>, SourceError> {
    let link_selector = selector("a[href]")?;
    let mut assets = Vec::new();

    for anchor in scope.select(&link_selector) {
        let href = anchor.value().attr("href").unwrap_or_default().trim();
        if href.is_empty() {
            continue;
        }

        let label = clean_text(&anchor.text().collect::<Vec<_>>().join(" "));
        let absolute = absolutize(base_url, href);
        let lower_url = absolute.to_ascii_lowercase();

        if lower_url.ends_with(".pdf") && is_repository_asset_url(href, &absolute) {
            let canonical = canonical_repository_url(&absolute, source_id, document_url);
            assets.push(SourceAsset {
                asset_url: canonical,
                label: if label.is_empty() {
                    "NOAA Repository PDF".to_owned()
                } else {
                    label
                },
                mime_type: Some("application/pdf".to_owned()),
                role: SourceAssetRole::Pdf,
            });
            continue;
        }

        if lower_url.ends_with(".pdf") {
            assets.push(SourceAsset {
                asset_url: absolute,
                label: if label.is_empty() {
                    "Source PDF".to_owned()
                } else {
                    label
                },
                mime_type: Some("application/pdf".to_owned()),
                role: SourceAssetRole::Other,
            });
            continue;
        }

        if lower_url.starts_with("https://repository.library.noaa.gov/view/noaa/")
            || lower_url.starts_with("http://repository.library.noaa.gov/view/noaa/")
        {
            assets.push(SourceAsset {
                asset_url: canonical_repository_url(&absolute, source_id, document_url),
                label: if label.is_empty() {
                    "Repository Metadata Page".to_owned()
                } else {
                    label
                },
                mime_type: Some("text/html".to_owned()),
                role: SourceAssetRole::Html,
            });
            continue;
        }

        if lower_url.contains("doi.org/") || label.to_ascii_lowercase().contains("source") {
            assets.push(SourceAsset {
                asset_url: absolute,
                label: if label.is_empty() {
                    "Source URL".to_owned()
                } else {
                    label
                },
                mime_type: None,
                role: SourceAssetRole::Other,
            });
        }
    }

    Ok(assets)
}

fn canonical_repository_url(url: &str, source_id: &str, document_url: &str) -> String {
    if url.starts_with("https://repository.library.noaa.gov/") {
        return url.to_owned();
    }

    if url.starts_with("http://repository.library.noaa.gov/") {
        return format!("https://{}", &url["http://".len()..]);
    }

    if let Some((_, tail)) = url.split_once("/view/noaa/") {
        return format!("https://repository.library.noaa.gov/view/noaa/{tail}");
    }

    if url.to_ascii_lowercase().ends_with(".pdf") {
        return format!("{document_url}/noaa_{source_id}_DS1.pdf");
    }

    url.to_owned()
}

fn is_repository_asset_url(href: &str, absolute_url: &str) -> bool {
    let lower_href = href.to_ascii_lowercase();
    if lower_href.starts_with("/view/noaa/") || lower_href.starts_with("view/noaa/") {
        return true;
    }

    let lower_url = absolute_url.to_ascii_lowercase();
    lower_url.starts_with("https://repository.library.noaa.gov/view/noaa/")
        || lower_url.starts_with("http://repository.library.noaa.gov/view/noaa/")
}

fn base_metadata(source_id: &str, document_url: &str) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    metadata.insert("repository_id".to_owned(), source_id.to_owned());
    metadata.insert(
        "official_repository_url".to_owned(),
        document_url.to_owned(),
    );
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata
}
