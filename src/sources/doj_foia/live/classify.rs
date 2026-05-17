use crate::sources::{SourceAsset, SourceAssetRole};

pub(crate) fn is_candidate_component_link(url: &str, link_text: &str) -> bool {
    let trimmed = link_text.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return false;
    }
    if !looks_like_component_name(trimmed) {
        return false;
    }

    let lower = url.to_ascii_lowercase();
    lower.contains("foia")
        || lower.contains("readingroom")
        || lower.contains("reading-room")
        || lower.contains("elect-read-room")
        || lower.contains("available-documents")
}

pub(crate) fn looks_like_component_name(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if [
        "foia",
        "foia library",
        "about",
        "archives",
        "privacy",
        "contact",
        "budget",
    ]
    .contains(&lower.as_str())
    {
        return false;
    }

    [
        "office",
        "division",
        "bureau",
        "service",
        "administration",
        "commission",
        "interpol",
        "attorney",
        "criminal",
        "civil",
        "marshals",
        "tax",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

pub(crate) fn disclosure_category_for_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("readingroom")
        || lower.contains("reading-room")
        || lower.contains("elect-read-room")
    {
        "foia_reading_room"
    } else if lower.contains("foia-library") || lower.contains("foia/library") {
        "foia_library"
    } else if lower.contains("available-documents") || lower.contains("foia") {
        "proactive_disclosure"
    } else {
        "component_disclosure"
    }
}

pub(crate) fn description_for_category(category: &str) -> &'static str {
    match category {
        "foia_reading_room" => {
            "DOJ component FOIA reading-room lead from the OIP all-components index. Verify official component context and publication date before citing."
        }
        "foia_library" => {
            "DOJ component FOIA library lead from the OIP all-components index. Verify official component context and publication date before citing."
        }
        "proactive_disclosure" => {
            "DOJ proactive-disclosure lead from the OIP all-components index. Verify the official component page and linked document context before citing."
        }
        _ => {
            "DOJ component disclosure lead from official OIP indexing. Verify component context and publication date on the official page before citing."
        }
    }
}

pub(crate) fn is_candidate_asset_link(url: &str, link_text: &str) -> bool {
    let lower_url = url.to_ascii_lowercase();
    if lower_url.ends_with(".pdf") || lower_url.contains(".pdf?") {
        return true;
    }
    if lower_url.ends_with(".html") || lower_url.ends_with(".htm") {
        return true;
    }

    let lower_text = link_text.to_ascii_lowercase();
    lower_text.contains("foia request log")
        || lower_text.contains("reading room")
        || lower_text.contains("foia library")
        || lower_text.contains("annual report")
}

pub(crate) fn asset_is_pdf(asset: &SourceAsset) -> bool {
    asset.role == SourceAssetRole::Pdf
}
