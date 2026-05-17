use scraper::Html;

use crate::sources::{SourceAsset, SourceAssetRole, SourceError, SourceMetadata, SourceRecord};

use super::assets::dedupe_and_sort_assets;
use super::html::{clean_text, ensure_html_body, first_text_from_document, meta_content, selector};
use super::url::{
    absolutize, document_key, is_official_pdf_url, normalize_accession, official_citation_url,
    source_id_from_official_url,
};
use super::{dtic_citation_note, dtic_terms_note, DTIC_SOURCE};

const SOURCE_WARNING: &str = "DTIC public access/search behavior is fragile. Verify accession ids, distribution statements, and official DTIC URLs before publication.";

pub(crate) fn record_from_detail_html(
    body: &str,
    base_url: &str,
    origin_url: &str,
    accession_hint: Option<&str>,
) -> Result<SourceRecord, SourceError> {
    ensure_html_body(body, origin_url)?;

    let document = Html::parse_document(body);
    let accession = accession_hint
        .and_then(normalize_accession)
        .or_else(|| source_id_from_official_url(origin_url))
        .or_else(|| extract_accession_from_html(&document))
        .ok_or_else(|| SourceError::SourceChanged {
            source: DTIC_SOURCE,
            message: "DTIC citation detail did not expose a stable accession id.".to_owned(),
            url: Some(origin_url.to_owned()),
        })?;

    let title = first_text_from_document(&document, &["h1", ".record-title", ".citation-title"])
        .or_else(|| {
            meta_content(
                &document,
                &["meta[property='og:title']", "meta[name='title']"],
            )
        })
        .ok_or_else(|| SourceError::SourceChanged {
            source: DTIC_SOURCE,
            message: "DTIC citation detail is missing an expected title field.".to_owned(),
            url: Some(origin_url.to_owned()),
        })?;

    let pairs = metadata_pairs(&document)?;
    let report_number =
        metadata_value(&pairs, &["report number", "report no", "report identifier"]);
    let date = metadata_value(&pairs, &["report date", "publication date", "date"]);
    let authors = metadata_value(&pairs, &["author", "authors", "personal author"]);
    let corporate_author = metadata_value(
        &pairs,
        &[
            "corporate author",
            "sponsoring agency",
            "performing organization",
        ],
    );
    let subject_terms = metadata_value(&pairs, &["subject terms", "descriptors", "keywords"]);
    let distribution_statement = metadata_value(
        &pairs,
        &[
            "distribution statement",
            "distribution",
            "distribution notes",
        ],
    );
    let abstract_text = metadata_value(&pairs, &["abstract", "description", "summary"]);

    let citation_url = official_citation_url(&accession);
    let mut attachments = extract_assets(&document, base_url, &citation_url)?;
    attachments.push(SourceAsset {
        asset_url: citation_url.clone(),
        label: "DTIC Citation Landing Page".to_owned(),
        mime_type: Some("text/html".to_owned()),
        role: SourceAssetRole::Html,
    });
    attachments = dedupe_and_sort_assets(attachments);

    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    let mut metadata = SourceMetadata::new();
    metadata.insert("dtic_accession".to_owned(), accession.clone());
    metadata.insert("official_citation_url".to_owned(), citation_url.clone());
    metadata.insert("source_warning".to_owned(), SOURCE_WARNING.to_owned());
    metadata.insert("asset_count".to_owned(), attachments.len().to_string());
    metadata.insert(
        "pdf_asset_count".to_owned(),
        attachments
            .iter()
            .filter(|asset| asset.role == SourceAssetRole::Pdf)
            .count()
            .to_string(),
    );
    if let Some(url) = pdf_url.as_deref() {
        metadata.insert("official_pdf_url".to_owned(), url.to_owned());
    }
    if let Some(value) = report_number.as_deref() {
        metadata.insert("report_number".to_owned(), value.to_owned());
    }
    if let Some(value) = authors.as_deref() {
        metadata.insert("authors".to_owned(), value.to_owned());
    }
    if let Some(value) = corporate_author.as_deref() {
        metadata.insert("corporate_author".to_owned(), value.to_owned());
    }
    if let Some(value) = subject_terms.as_deref() {
        metadata.insert("subject_terms".to_owned(), value.to_owned());
    }
    if let Some(value) = distribution_statement.as_deref() {
        metadata.insert("distribution_statement".to_owned(), value.to_owned());
    }
    if let Some(value) = abstract_text.as_deref() {
        metadata.insert("abstract".to_owned(), value.to_owned());
    }

    Ok(SourceRecord {
        id: format!("{DTIC_SOURCE}:{accession}"),
        document_key: document_key(&accession),
        source: DTIC_SOURCE,
        source_id: accession,
        title,
        date,
        collection: Some("Defense Technical Information Center (DTIC)".to_owned()),
        record_group: corporate_author.clone(),
        description: abstract_text,
        origin_url: origin_url.to_owned(),
        document_url: citation_url,
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(dtic_citation_note().to_owned()),
        terms_note: Some(dtic_terms_note().to_owned()),
    })
}

fn metadata_pairs(document: &Html) -> Result<Vec<(String, String)>, SourceError> {
    let mut pairs = Vec::new();

    let row_selector = selector("table tr")?;
    let th_selector = selector("th")?;
    let td_selector = selector("td")?;
    for row in document.select(&row_selector) {
        let label = row
            .select(&th_selector)
            .next()
            .or_else(|| row.select(&td_selector).next())
            .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
        let value = row
            .select(&td_selector)
            .nth(1)
            .or_else(|| row.select(&td_selector).next())
            .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();

        if !label.is_empty() && !value.is_empty() {
            pairs.push((label, value));
        }
    }

    let dl_selector = selector("dl")?;
    let dt_selector = selector("dt")?;
    let dd_selector = selector("dd")?;
    for block in document.select(&dl_selector) {
        let labels = block
            .select(&dt_selector)
            .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")))
            .collect::<Vec<_>>();
        let values = block
            .select(&dd_selector)
            .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")))
            .collect::<Vec<_>>();
        for (label, value) in labels.into_iter().zip(values.into_iter()) {
            if !label.is_empty() && !value.is_empty() {
                pairs.push((label, value));
            }
        }
    }

    Ok(pairs)
}

fn metadata_value(pairs: &[(String, String)], aliases: &[&str]) -> Option<String> {
    for (label, value) in pairs {
        let normalized = label.to_ascii_lowercase();
        if aliases.iter().any(|alias| normalized.contains(alias)) {
            return Some(value.clone());
        }
    }
    None
}

fn extract_assets(
    document: &Html,
    base_url: &str,
    citation_url: &str,
) -> Result<Vec<SourceAsset>, SourceError> {
    let link_selector = selector("a[href]")?;
    let mut assets = Vec::new();

    for link in document.select(&link_selector) {
        let href = link.value().attr("href").unwrap_or_default();
        let absolute = absolutize(base_url, href);
        let label = clean_text(&link.text().collect::<Vec<_>>().join(" "));
        let normalized_label = if label.is_empty() {
            "DTIC Linked Asset".to_owned()
        } else {
            label
        };

        let lower = absolute.to_ascii_lowercase();
        let role = if is_official_pdf_url(&absolute) {
            SourceAssetRole::Pdf
        } else if absolute == citation_url || lower.contains("dtic.mil") {
            SourceAssetRole::Html
        } else {
            SourceAssetRole::Other
        };

        let mime_type = match role {
            SourceAssetRole::Pdf => Some("application/pdf".to_owned()),
            SourceAssetRole::Html => Some("text/html".to_owned()),
            _ => None,
        };

        assets.push(SourceAsset {
            asset_url: absolute,
            label: normalized_label,
            mime_type,
            role,
        });
    }

    Ok(assets)
}

fn extract_accession_from_html(document: &Html) -> Option<String> {
    let text = clean_text(&document.root_element().text().collect::<Vec<_>>().join(" "));
    for token in text.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        if let Some(accession) = normalize_accession(token) {
            return Some(accession);
        }
    }
    None
}
