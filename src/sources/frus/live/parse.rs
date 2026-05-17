use crate::sources::SourceError;

use super::url::{
    is_document_source_id, official_document_url_for_source_id,
    source_id_from_historicaldocuments_path,
};
use super::{FRUS_SOURCE, SOURCE_CHANGED_WARNING};

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedFrusRecord {
    pub source_id: String,
    pub volume_id: Option<String>,
    pub element_id: Option<String>,
    pub volume_title: Option<String>,
    pub document_title: String,
    pub document_number: Option<String>,
    pub date: Option<String>,
    pub official_url: String,
    pub official_volume_url: Option<String>,
    pub tei_url: Option<String>,
    pub pdf_url: Option<String>,
    pub ebook_url: Option<String>,
    pub summary: Option<String>,
    pub persons: Vec<String>,
    pub places: Vec<String>,
    pub topics: Vec<String>,
}

pub(crate) fn records_from_search_html(
    body: &str,
    search_url: &str,
) -> Result<Vec<ParsedFrusRecord>, SourceError> {
    if !contains_case_insensitive(body, "hsg-search-results") {
        return Err(SourceError::SourceChanged {
            source: FRUS_SOURCE,
            message: "FRUS search returned an unexpected non-search HTML response.".to_owned(),
            url: Some(search_url.to_owned()),
        });
    }

    let mut records = Vec::new();
    for block in split_search_result_blocks(body) {
        let Some((href, label)) = anchors(block).into_iter().next() else {
            continue;
        };
        let Some(source_id) = source_id_from_historicaldocuments_path(&href) else {
            continue;
        };
        if !is_document_source_id(&source_id) {
            continue;
        }

        let mut record = base_record_for_source_id(&source_id);
        record.document_title = clean_search_title(&label);
        record.date = detail_value(block, "Recorded Date");
        record.summary = Some(clean_html_text(block));
        if let Some(volume_title) = volume_title_from_search_label(&label) {
            record.volume_title = Some(volume_title);
        }
        records.push(record);
    }

    Ok(records)
}

pub(crate) fn record_from_detail_html(
    body: &str,
    request_url: &str,
    source_id_hint: Option<&str>,
) -> Result<ParsedFrusRecord, SourceError> {
    if !contains_case_insensitive(body, "content-inner")
        || !contains_case_insensitive(body, "tei-div")
    {
        return Err(SourceError::SourceChanged {
            source: FRUS_SOURCE,
            message: SOURCE_CHANGED_WARNING.to_owned(),
            url: Some(request_url.to_owned()),
        });
    }

    let source_id = source_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| source_id_from_historicaldocuments_path(request_url))
        .ok_or_else(|| SourceError::SourceChanged {
            source: FRUS_SOURCE,
            message: "FRUS detail page did not expose a document source id.".to_owned(),
            url: Some(request_url.to_owned()),
        })?;

    let mut record = base_record_for_source_id(&source_id);
    record.volume_title = non_empty(first_tag_text(body, "h2")).or(record.volume_title);
    record.document_title = non_empty(first_tag_text(body, "h3"))
        .map(clean_document_title)
        .unwrap_or_else(|| record.document_title.clone());
    record.document_number = document_number_from_title(&record.document_title)
        .or_else(|| record.document_number.clone());
    record.date = first_span_with_class(body, "tei-date");
    record.places = spans_with_class(body, "tei-placeName");
    record.persons = spans_with_class(body, "tei-persName");
    record.topics = spans_with_class(body, "tei-gloss");

    if let Some(source_note) = first_footnote_source(body) {
        record.summary = Some(source_note);
    }

    if record.document_title.trim().is_empty() {
        return Err(SourceError::SourceChanged {
            source: FRUS_SOURCE,
            message: SOURCE_CHANGED_WARNING.to_owned(),
            url: Some(request_url.to_owned()),
        });
    }

    Ok(record)
}

fn base_record_for_source_id(source_id: &str) -> ParsedFrusRecord {
    let normalized = source_id.trim().trim_start_matches("frus:").trim();
    let mut parts = normalized.split('/');
    let volume_id = parts.next().unwrap_or_default().to_owned();
    let element_id = parts.next().unwrap_or_default().to_owned();
    let document_number = document_number_from_element_id(&element_id);

    ParsedFrusRecord {
        source_id: normalized.to_owned(),
        volume_id: Some(volume_id.clone()),
        element_id: Some(element_id),
        volume_title: None,
        document_title: document_number
            .as_deref()
            .map(|number| format!("Document {number}"))
            .unwrap_or_else(|| normalized.to_owned()),
        document_number,
        date: None,
        official_url: official_document_url_for_source_id(normalized),
        official_volume_url: Some(format!(
            "https://history.state.gov/historicaldocuments/{volume_id}"
        )),
        tei_url: Some(format!(
            "https://raw.githubusercontent.com/HistoryAtState/frus/master/volumes/{volume_id}.xml"
        )),
        pdf_url: Some(format!(
            "https://static.history.state.gov/frus/{volume_id}/pdf/{volume_id}.pdf"
        )),
        ebook_url: Some(format!(
            "https://static.history.state.gov/frus/{volume_id}/ebook/{volume_id}.epub"
        )),
        summary: None,
        persons: Vec::new(),
        places: Vec::new(),
        topics: Vec::new(),
    }
}

fn split_search_result_blocks(html: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut cursor = html;
    while let Some(start) = find_case_insensitive(cursor, "<div class=\"hsg-search-result\"") {
        let after_start = &cursor[start..];
        let next_start =
            find_case_insensitive(&after_start[1..], "<div class=\"hsg-search-result\"")
                .map(|index| index + 1)
                .unwrap_or(after_start.len());
        blocks.push(&after_start[..next_start]);
        cursor = &after_start[next_start..];
    }
    blocks
}

fn anchors(html: &str) -> Vec<(String, String)> {
    let mut values = Vec::new();
    let mut cursor = html;
    while let Some(start) = find_case_insensitive(cursor, "<a") {
        let after_start = &cursor[start..];
        let Some(tag_end) = after_start.find('>') else {
            break;
        };
        let tag = &after_start[..=tag_end];
        let Some(href) = attr_value(tag, "href") else {
            cursor = &after_start[tag_end + 1..];
            continue;
        };
        let content = &after_start[tag_end + 1..];
        let Some(close) = find_case_insensitive(content, "</a>") else {
            break;
        };
        values.push((href.trim().to_owned(), clean_html_text(&content[..close])));
        cursor = &content[close + "</a>".len()..];
    }
    values
}

fn detail_value(html: &str, label: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let label_lower = label.to_ascii_lowercase();
    let dt_start = lower.find("<dt")?;
    let mut cursor = &html[dt_start..];
    loop {
        let open_end = cursor.find('>')?;
        let content = &cursor[open_end + 1..];
        let close = find_case_insensitive(content, "</dt>")?;
        if clean_html_text(&content[..close]).eq_ignore_ascii_case(&label_lower) {
            let after_dt = &content[close + "</dt>".len()..];
            let dd_start = find_case_insensitive(after_dt, "<dd")?;
            let dd = &after_dt[dd_start..];
            let dd_open_end = dd.find('>')?;
            let dd_content = &dd[dd_open_end + 1..];
            let dd_close = find_case_insensitive(dd_content, "</dd>")?;
            return non_empty(clean_html_text(&dd_content[..dd_close]));
        }
        let after_dt = &content[close + "</dt>".len()..];
        let next_dt = find_case_insensitive(after_dt, "<dt")?;
        cursor = &after_dt[next_dt..];
    }
}

fn first_tag_text(html: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let Some(start) = find_case_insensitive(html, &open) else {
        return String::new();
    };
    let tail = &html[start..];
    let Some(open_end) = tail.find('>') else {
        return String::new();
    };
    let content = &tail[open_end + 1..];
    let Some(close_start) = find_case_insensitive(content, &close) else {
        return String::new();
    };
    clean_html_text(&content[..close_start])
}

fn first_span_with_class(html: &str, class_name: &str) -> Option<String> {
    spans_with_class(html, class_name).into_iter().next()
}

fn spans_with_class(html: &str, class_name: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut cursor = html;
    while let Some(start) = find_case_insensitive(cursor, "<span") {
        let after_start = &cursor[start..];
        let Some(tag_end) = after_start.find('>') else {
            break;
        };
        let tag = &after_start[..=tag_end];
        let class_matches = attr_value(tag, "class")
            .map(|value| value.split_whitespace().any(|item| item == class_name))
            .unwrap_or(false);
        let content = &after_start[tag_end + 1..];
        let Some(close) = find_case_insensitive(content, "</span>") else {
            break;
        };
        if class_matches {
            push_unique(&mut values, &clean_html_text(&content[..close]));
        }
        cursor = &content[close + "</span>".len()..];
    }
    values
}

fn first_footnote_source(html: &str) -> Option<String> {
    let source_index = find_case_insensitive(html, "Source:")?;
    let tail = &html[source_index..];
    let end = find_case_insensitive(tail, "</span>")
        .or_else(|| find_case_insensitive(tail, "</li>"))
        .unwrap_or(tail.len());
    non_empty(clean_html_text(&tail[..end]))
}

fn clean_search_title(label: &str) -> String {
    let title = label.trim();
    if let Some(index) = title.rfind(" (") {
        return title[..index].trim().to_owned();
    }
    title.to_owned()
}

fn clean_document_title(title: String) -> String {
    title
}

fn volume_title_from_search_label(label: &str) -> Option<String> {
    let start = label.rfind('(')? + 1;
    let end = label.rfind(')')?;
    (start < end).then(|| label[start..end].trim().to_owned())
}

fn document_number_from_title(title: &str) -> Option<String> {
    let prefix = title.split('.').next()?.trim();
    (!prefix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit())).then(|| prefix.to_owned())
}

fn document_number_from_element_id(element_id: &str) -> Option<String> {
    element_id
        .strip_prefix('d')
        .filter(|value| !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()))
        .map(ToOwned::to_owned)
}

fn clean_html_text(value: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }

    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn attr_value<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{attr}=");
    let attr_index = lower.find(&needle)? + needle.len();
    let remainder = &tag[attr_index..];
    let quote = remainder.chars().next()?;
    if quote == '"' || quote == '\'' {
        let value_start = quote.len_utf8();
        let value_end = remainder[value_start..].find(quote)? + value_start;
        Some(&remainder[value_start..value_end])
    } else {
        let value_end = remainder
            .find([' ', '>', '\t', '\n', '\r'])
            .unwrap_or(remainder.len());
        Some(&remainder[..value_end])
    }
}

fn push_unique(items: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() && !items.iter().any(|item| item == value) {
        items.push(value.to_owned());
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    find_case_insensitive(haystack, needle).is_some()
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}
