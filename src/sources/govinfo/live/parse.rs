use crate::sources::{SourceAssetRole, SourceError, SourceMetadata, SourceRecord};
use serde_json::Value;

use super::assets::attachments_from_download;
use super::transport::percent_encode_path_segment;
use super::{
    govinfo_citation_note, govinfo_terms_note, GovInfoLocator, GOVINFO_SEARCH_OVERVIEW_URL,
    GOVINFO_SOURCE,
};

pub(crate) fn parse_locator(id_or_url: &str) -> Result<GovInfoLocator, SourceError> {
    let mut value = id_or_url.trim();
    if value.is_empty() {
        return Err(SourceError::invalid_input(
            GOVINFO_SOURCE,
            "GovInfo record lookup expects a non-empty package id, package/granule id, details URL, or summary URL.",
            Some(
                "Examples: USREPORTS-99, USREPORTS-99/USREPORTS-99-FrontMatter-2, https://www.govinfo.gov/app/details/USREPORTS-99, or https://api.govinfo.gov/packages/USREPORTS-99/summary."
                    .to_owned(),
            ),
        ));
    }

    if let Some(stripped) = value.strip_prefix("govinfo:") {
        value = stripped.trim();
    }

    if let Some(locator) = locator_from_api_url(value) {
        return Ok(locator);
    }
    if let Some(locator) = locator_from_details_url(value) {
        return Ok(locator);
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        return Err(SourceError::invalid_input(
            GOVINFO_SOURCE,
            "GovInfo URL format is not recognized.",
            Some(
                "Use official GovInfo summary URLs (/packages/{PACKAGE_ID}/summary or /packages/{PACKAGE_ID}/granules/{GRANULE_ID}/summary), details URLs (/app/details/{PACKAGE_ID}[/{GRANULE_ID}]), or package ids."
                    .to_owned(),
            ),
        ));
    }

    if let Some((package_id, granule_id)) = value.split_once('/') {
        let package_id = package_id.trim();
        let granule_id = granule_id.trim();
        if package_id.is_empty() || granule_id.is_empty() {
            return Err(SourceError::invalid_input(
                GOVINFO_SOURCE,
                "GovInfo composite id must include both package and granule ids.",
                Some("Use format {PACKAGE_ID}/{GRANULE_ID}.".to_owned()),
            ));
        }
        return Ok(GovInfoLocator::Granule {
            package_id: package_id.to_owned(),
            granule_id: granule_id.to_owned(),
        });
    }

    Ok(GovInfoLocator::Package {
        package_id: value.to_owned(),
    })
}

pub(crate) fn record_from_search_result(result: &Value) -> Option<SourceRecord> {
    let package_id = first_string(result, &["packageId"])?;
    let granule_id = first_string(result, &["granuleId"]);
    let source_id = match granule_id.as_deref() {
        Some(granule_id) => format!("{package_id}/{granule_id}"),
        None => package_id.clone(),
    };

    let details_link = details_link_for(&package_id, granule_id.as_deref());
    let result_link = first_string(result, &["resultLink"]);
    let document_url = result_link.clone().unwrap_or_else(|| details_link.clone());
    let metadata = metadata_from_fields(
        result,
        &[
            "collectionCode",
            "dateIssued",
            "dateIngested",
            "lastModified",
            "relatedLink",
            "resultLink",
        ],
    );

    let attachments = attachments_from_download(result.get("download"));
    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    let text_preview = strings_from_array(result.get("governmentAuthor"));

    Some(SourceRecord {
        id: format!("{GOVINFO_SOURCE}:{source_id}"),
        document_key: document_key(GOVINFO_SOURCE, &source_id),
        source: GOVINFO_SOURCE,
        source_id,
        title: first_string(result, &["title"]).unwrap_or_else(|| package_id.clone()),
        date: first_string(result, &["dateIssued"]),
        collection: first_string(result, &["collectionCode"]),
        record_group: None,
        description: None,
        origin_url: details_link,
        document_url,
        pdf_url,
        metadata,
        attachments,
        text_preview,
        citation_note: Some(govinfo_citation_note().to_owned()),
        terms_note: Some(govinfo_terms_note().to_owned()),
    })
}

pub(crate) fn record_from_summary(
    payload: &Value,
    locator: &GovInfoLocator,
) -> Result<SourceRecord, SourceError> {
    let fallback_source_id = locator.source_id();
    let package_id = first_string(payload, &["packageId"]).unwrap_or_else(|| match locator {
        GovInfoLocator::Package { package_id } => package_id.clone(),
        GovInfoLocator::Granule { package_id, .. } => package_id.clone(),
    });
    let granule_id = first_string(payload, &["granuleId"]).or_else(|| match locator {
        GovInfoLocator::Granule { granule_id, .. } => Some(granule_id.clone()),
        GovInfoLocator::Package { .. } => None,
    });

    let source_id = match granule_id.as_deref() {
        Some(granule_id) => format!("{package_id}/{granule_id}"),
        None => package_id.clone(),
    };

    if source_id.trim().is_empty() {
        return Err(SourceError::SourceChanged {
            source: GOVINFO_SOURCE,
            message: "GovInfo summary response is missing package identity fields.".to_owned(),
            url: Some(GOVINFO_SEARCH_OVERVIEW_URL.to_owned()),
        });
    }

    let details_link = first_string(payload, &["detailsLink"])
        .unwrap_or_else(|| details_link_for(&package_id, granule_id.as_deref()));
    let package_link = first_string(payload, &["packageLink"]);
    let document_url = package_link
        .or_else(|| first_string(payload, &["granulesLink"]))
        .unwrap_or_else(|| details_link.clone());
    let metadata = metadata_from_fields(
        payload,
        &[
            "collectionCode",
            "collectionName",
            "dateIssued",
            "lastModified",
            "category",
            "docClass",
            "granuleClass",
            "packageLink",
            "granulesLink",
            "relatedLink",
            "pages",
        ],
    );
    let attachments = attachments_from_download(payload.get("download"));
    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());

    Ok(SourceRecord {
        id: format!("{GOVINFO_SOURCE}:{source_id}"),
        document_key: document_key(GOVINFO_SOURCE, &source_id),
        source: GOVINFO_SOURCE,
        source_id: if source_id.is_empty() {
            fallback_source_id
        } else {
            source_id
        },
        title: first_string(payload, &["title"]).unwrap_or_else(|| package_id.clone()),
        date: first_string(payload, &["dateIssued"]),
        collection: first_string(payload, &["collectionCode", "collectionName"]),
        record_group: None,
        description: None,
        origin_url: details_link,
        document_url,
        pdf_url,
        metadata,
        attachments,
        text_preview: None,
        citation_note: Some(govinfo_citation_note().to_owned()),
        terms_note: Some(govinfo_terms_note().to_owned()),
    })
}

impl GovInfoLocator {
    pub(crate) fn source_id(&self) -> String {
        match self {
            GovInfoLocator::Package { package_id } => package_id.clone(),
            GovInfoLocator::Granule {
                package_id,
                granule_id,
            } => format!("{package_id}/{granule_id}"),
        }
    }
}

fn locator_from_api_url(url: &str) -> Option<GovInfoLocator> {
    let after_packages = url.split("/packages/").nth(1)?;
    let path = after_packages.split(['?', '#']).next()?.trim_matches('/');
    if path.is_empty() {
        return None;
    }

    if let Some((package_id, granule_tail)) = path.split_once("/granules/") {
        let package_id = package_id.trim();
        let granule_tail = granule_tail.trim();
        if package_id.is_empty() || granule_tail.is_empty() {
            return None;
        }
        let granule_id = granule_tail.trim_end_matches("/summary").trim_matches('/');
        if granule_id.is_empty() {
            return None;
        }
        return Some(GovInfoLocator::Granule {
            package_id: package_id.to_owned(),
            granule_id: granule_id.to_owned(),
        });
    }

    let package_id = path.trim_end_matches("/summary").trim_matches('/');
    if package_id.is_empty() {
        return None;
    }

    Some(GovInfoLocator::Package {
        package_id: package_id.to_owned(),
    })
}

fn locator_from_details_url(url: &str) -> Option<GovInfoLocator> {
    let after_details = url.split("/app/details/").nth(1)?;
    let path = after_details.split(['?', '#']).next()?.trim_matches('/');
    if path.is_empty() {
        return None;
    }

    let mut segments = path.split('/');
    let package_id = segments.next()?.trim();
    if package_id.is_empty() {
        return None;
    }

    let granule_id = segments
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(granule_id) = granule_id {
        Some(GovInfoLocator::Granule {
            package_id: package_id.to_owned(),
            granule_id: granule_id.to_owned(),
        })
    } else {
        Some(GovInfoLocator::Package {
            package_id: package_id.to_owned(),
        })
    }
}

fn details_link_for(package_id: &str, granule_id: Option<&str>) -> String {
    match granule_id {
        Some(granule_id) => format!(
            "https://www.govinfo.gov/app/details/{}/{}",
            percent_encode_path_segment(package_id),
            percent_encode_path_segment(granule_id)
        ),
        None => format!(
            "https://www.govinfo.gov/app/details/{}",
            percent_encode_path_segment(package_id)
        ),
    }
}

fn metadata_from_fields(value: &Value, fields: &[&str]) -> SourceMetadata {
    let mut metadata = SourceMetadata::new();
    for field in fields {
        if let Some(string_value) = first_string(value, &[*field]) {
            if !string_value.trim().is_empty() {
                metadata.insert((*field).to_owned(), string_value);
            }
        }
    }
    metadata
}

fn strings_from_array(value: Option<&Value>) -> Option<String> {
    let mut values = Vec::new();
    let items = value.and_then(Value::as_array)?;
    for item in items {
        if let Some(text) = item.as_str() {
            let text = text.trim();
            if !text.is_empty() {
                values.push(text.to_owned());
            }
        }
    }
    if values.is_empty() {
        None
    } else {
        Some(values.join("; "))
    }
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(node) = value.get(*key) else {
            continue;
        };
        if let Some(string) = node.as_str() {
            let trimmed = string.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
            continue;
        }
        if node.is_number() || node.is_boolean() {
            return Some(node.to_string());
        }
    }
    None
}

fn document_key(source: &str, source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in source.bytes().chain([b':']).chain(source_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{source}-{hash:016x}")
}
