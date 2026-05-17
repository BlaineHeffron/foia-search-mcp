use super::asset::looks_like_part_label;

pub(crate) fn is_candidate_record_link(url: &str, text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }

    let lower_url = url.to_ascii_lowercase();
    let lower_text = text.to_ascii_lowercase();

    if lower_url.contains("/search")
        || lower_url.contains("/fdps-")
        || lower_url.contains("/foia")
        || lower_url.contains("/check-status")
    {
        return false;
    }

    if matches!(
        lower_text.as_str(),
        "home"
            | "about"
            | "about the vault"
            | "a-z index"
            | "proactive disclosures"
            | "next 20 items »"
            | "vault home"
            | "search"
    ) {
        return false;
    }

    if lower_url.ends_with("/at_download/file") || lower_url.ends_with("/view") {
        return looks_like_part_label(&lower_text);
    }

    true
}

pub(crate) fn is_candidate_asset_link(url: &str, label: &str) -> bool {
    let lower_url = url.to_ascii_lowercase();
    let lower_label = label.to_ascii_lowercase();

    lower_url.ends_with("/at_download/file")
        || lower_url.ends_with(".pdf")
        || lower_url.contains(".pdf?")
        || (lower_url.ends_with("/view") && looks_like_part_label(&lower_label))
        || lower_url.ends_with(".jpg")
        || lower_url.ends_with(".jpeg")
        || lower_url.ends_with(".png")
        || lower_url.ends_with(".gif")
        || lower_label.contains("download pdf")
}

pub(crate) fn category_for_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("proactive-disclosure") {
        "proactive_disclosure"
    } else if lower.contains("discretionary") {
        "discretionary_release"
    } else if lower.contains("all-files") {
        "all_files"
    } else {
        "vault_file"
    }
}

pub(crate) fn description_for_category(category: &str) -> &'static str {
    match category {
        "proactive_disclosure" => {
            "FBI proactive disclosure lead. Verify official Vault release context and part ordering before citation."
        }
        "discretionary_release" => {
            "FBI discretionary release lead referenced from Vault context. Verify official release context and provenance before citation."
        }
        "all_files" => {
            "FBI Vault index lead. Follow to the specific file page and preserve multipart ordering before ingesting PDFs."
        }
        _ => {
            "FBI Vault file/collection lead. Verify official page context and multipart ordering before citation or ingestion."
        }
    }
}

pub(crate) fn title_for_search_result(text: &str, source_id: &str) -> String {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        return trimmed.to_owned();
    }

    source_id
        .split('/')
        .next_back()
        .unwrap_or(source_id)
        .split('-')
        .filter(|segment| !segment.is_empty())
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn collection_name_from_source_id(source_id: &str) -> String {
    let mut segments = source_id.split('/').filter(|segment| !segment.is_empty());
    if let Some(first) = segments.next() {
        first
            .split('-')
            .filter(|segment| !segment.is_empty())
            .map(capitalize_word)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        "FBI Vault".to_owned()
    }
}

fn capitalize_word(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}
