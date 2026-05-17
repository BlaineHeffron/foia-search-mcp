use std::collections::BTreeMap;

use crate::sources::{SourceMetadata, SourceRecord};

pub(crate) fn base_metadata(document_url: &str, category: &str, warning: &str) -> SourceMetadata {
    let mut metadata = BTreeMap::new();
    metadata.insert("library_section".to_owned(), "doj_disclosures".to_owned());
    metadata.insert("category".to_owned(), category.to_owned());
    metadata.insert("official_url".to_owned(), document_url.to_owned());
    metadata.insert("source_warning".to_owned(), warning.to_owned());
    metadata
}

pub(crate) fn enrich_metadata(
    metadata: &mut SourceMetadata,
    category: &str,
    source_id: &str,
    title: &str,
) {
    if category == "efta_data_set" {
        if let Some(number) = data_set_number(source_id, title) {
            metadata.insert("data_set".to_owned(), number);
        }
    }

    if category == "court_record" {
        metadata.insert("case_name".to_owned(), title.to_owned());
    }

    if category == "foia" {
        if let Some(component) = title.strip_prefix("FOIA:") {
            metadata.insert("component".to_owned(), component.trim().to_owned());
        } else if let Some(component) = source_id.strip_prefix("foia-") {
            metadata.insert(
                "component".to_owned(),
                component
                    .split('-')
                    .map(capitalize_word)
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
    }

    if category == "prior_doj_disclosure" {
        metadata.insert(
            "library_section".to_owned(),
            "prior_doj_disclosures".to_owned(),
        );
    }
}

pub(crate) fn record_matches_query(record: &SourceRecord, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {}",
        record.title,
        record.source_id,
        record.record_group.clone().unwrap_or_default(),
        record.description.clone().unwrap_or_default(),
        record.metadata.get("category").cloned().unwrap_or_default(),
        record
            .metadata
            .get("component")
            .cloned()
            .unwrap_or_default(),
    )
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|term| haystack.contains(&term.to_ascii_lowercase()))
}

pub(crate) fn data_set_number(source_id: &str, title: &str) -> Option<String> {
    if let Some(segment) = source_id.split("data-set-").nth(1) {
        let digits = segment
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if !digits.is_empty() {
            return Some(digits);
        }
    }

    let lower = title.to_ascii_lowercase();
    let marker = "data set ";
    let index = lower.find(marker)? + marker.len();
    let digits = lower[index..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty()).then_some(digits)
}

pub(crate) fn category_from_url_and_title(url: &str, title: &str) -> &'static str {
    let lower_url = url.to_ascii_lowercase();
    let lower_title = title.to_ascii_lowercase();

    if lower_url.contains("/data-set-") || lower_title.starts_with("data set") {
        "efta_data_set"
    } else if lower_url.contains("/court-records-") || lower_title.contains(" v. ") {
        "court_record"
    } else if lower_url.contains("/foia-") || lower_title.starts_with("foia:") {
        "foia"
    } else if lower_url.contains("/bop-video-footage")
        || lower_url.contains("/first-phase-declassified-epstein-files")
        || lower_url.contains("/maxwell-proffer")
        || lower_url.contains("/memoranda-and-correspondence")
    {
        "prior_doj_disclosure"
    } else {
        "release"
    }
}

pub(crate) fn description_for_category(category: &str) -> &'static str {
    match category {
        "efta_data_set" => {
            "EFTA DOJ disclosure data-set lead. Open the official DOJ page before selecting a PDF for ingestion."
        }
        "court_record" => {
            "Court-record lead in DOJ Epstein disclosures. Verify case context and redaction notes on the official DOJ page."
        }
        "foia" => {
            "FOIA component lead in DOJ Epstein disclosures. Confirm component provenance and redaction scope before citing."
        }
        "prior_doj_disclosure" => {
            "Prior DOJ disclosure lead. Mixed media may be present; PDFs are ingest-preferred."
        }
        _ => "DOJ Epstein disclosure lead. Verify official DOJ context and privacy notices before citing.",
    }
}

pub(crate) fn is_candidate_lead_link(url: &str, text: &str) -> bool {
    let lower_url = url.to_ascii_lowercase();
    let lower_text = text.to_ascii_lowercase();

    if lower_text.is_empty() {
        return false;
    }
    if matches!(
        lower_text.as_str(),
        "home"
            | "search full library"
            | "doj disclosures"
            | "house disclosures"
            | "facebook"
            | "x"
            | "linkedin"
            | "email"
            | "next"
            | "last"
            | "view files"
    ) {
        return lower_url.contains("/data-set-");
    }

    if lower_url.contains("/epstein/doj-disclosures/") {
        return true;
    }

    if lower_url.contains("/media/") {
        return lower_text.contains("memorandum")
            || lower_text.contains("press release")
            || lower_text.contains("letter");
    }

    false
}

pub(crate) fn is_asset_link(url: &str, text: &str) -> bool {
    let lower_url = url.to_ascii_lowercase();
    let lower_text = text.to_ascii_lowercase();

    if lower_url.contains("/epstein/files/") {
        return true;
    }

    if lower_url.ends_with(".pdf")
        || lower_url.ends_with(".jpg")
        || lower_url.ends_with(".jpeg")
        || lower_url.ends_with(".png")
        || lower_url.ends_with(".gif")
        || lower_url.ends_with(".webp")
        || lower_url.ends_with(".svg")
        || lower_url.ends_with(".mp4")
        || lower_url.ends_with(".mov")
        || lower_url.ends_with(".webm")
        || lower_url.ends_with(".mp3")
        || lower_url.ends_with(".wav")
        || lower_url.ends_with(".m4a")
    {
        return true;
    }

    if lower_text.starts_with("efta") && lower_text.ends_with(".pdf") {
        return true;
    }

    false
}

pub(crate) fn title_for_listing_lead(document_url: &str, text: &str) -> String {
    if !text.trim().is_empty() && text.trim() != "View files" {
        return text.trim().to_owned();
    }

    let source_id = super::parse::source_id_from_url(document_url);
    title_from_source_id(&source_id)
}

pub(crate) fn title_from_source_id(source_id: &str) -> String {
    source_id
        .split('-')
        .filter(|segment| !segment.is_empty())
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!(
        "{}{}",
        first.to_ascii_uppercase(),
        chars.collect::<String>().to_ascii_lowercase()
    )
}
