use std::collections::{BTreeMap, HashSet};

use super::{
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
    SourceError, SourceFuture, SourceRecord, SourceStatus,
};

pub const CIA_SOURCE: &str = "cia";
pub const CIA_SEARCH_SOURCE: &str = "cia_reading_room";
pub const DEFAULT_BASE_URL: &str = "https://www.cia.gov";
const SOURCE_CHANGED_WARNING: &str = "CIA Reading Room HTML shape may have changed or blocked scraping. Try the same query manually on the source site.";
const CITATION_NOTE: &str =
    "CIA FOIA Electronic Reading Room. Verify OCR and redactions against original scan/PDF.";

#[derive(Debug, Clone)]
pub struct CiaAdapter {
    base_url: String,
}

impl CiaAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn search_url(&self, query: &str, cursor: Option<&str>) -> String {
        let page = parse_cursor(cursor);
        let mut url = format!(
            "{}/readingroom/search/site/{}",
            self.base_url.trim_end_matches('/'),
            percent_encode_path_segment(query)
        );
        if page > 0 {
            url.push_str("?page=");
            url.push_str(&page.to_string());
        }
        url
    }

    pub fn document_url(&self, id_or_url: &str) -> Result<String, SourceError> {
        if id_or_url.starts_with("http://") || id_or_url.starts_with("https://") {
            if id_or_url.contains("/readingroom/document/") {
                return Ok(id_or_url.to_owned());
            }
            return Err(SourceError::invalid_input(
                CIA_SOURCE,
                "CIA document lookup expects a Reading Room document id or /readingroom/document/ URL.",
                Some("Pass ids like cia-rdp68r00530a000200110020-2.".to_owned()),
            ));
        }

        Ok(format!(
            "{}/readingroom/document/{}",
            self.base_url.trim_end_matches('/'),
            percent_encode_path_segment(id_or_url)
        ))
    }
}

impl Default for CiaAdapter {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }
}

impl SourceAdapter for CiaAdapter {
    fn name(&self) -> &'static str {
        CIA_SOURCE
    }

    fn status(&self) -> SourceStatus {
        SourceStatus::Enabled
    }

    fn search<'a>(
        &'a self,
        _query: &'a str,
        _options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move {
            Err(SourceError::Fetch {
                source: CIA_SOURCE,
                message:
                    "CIA network search is not wired until the MCP HTTP client scaffold lands."
                        .to_owned(),
                url: None,
            })
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            let url = self.document_url(id_or_url)?;
            Err(SourceError::Fetch {
                source: CIA_SOURCE,
                message: "CIA network record fetch is not wired until the MCP HTTP client scaffold lands."
                    .to_owned(),
                url: Some(url),
            })
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(record.attachments.clone()) })
    }

    fn cache_policy(&self) -> CachePolicy {
        CachePolicy::RespectSourceHeaders
    }
}

pub fn parse_cia_search(html: &str, base_url: &str, query: &str, page: usize) -> SearchPage {
    let mut records = Vec::new();

    for item in search_candidate_blocks(html) {
        let Some(href) = first_href_matching(&item, |href| href.contains("/readingroom/document/"))
        else {
            continue;
        };
        let url = absolutize(&href, base_url);
        let source_id = cia_document_id_from_url(&url).unwrap_or_else(|| url.clone());
        let title = first_non_empty(&[
            anchor_text_for_href(&item, &href),
            first_tag_text(&item, "h3"),
            first_tag_text(&item, "h2"),
            source_id.clone(),
        ]);
        let description = truncate_chars(&clean_html_text(&item), 500);
        let pdf_url = first_href_matching(&item, |href| href.to_ascii_lowercase().contains(".pdf"))
            .map(|href| absolutize(&href, base_url));
        let attachments = pdf_url
            .iter()
            .map(|url| SourceAsset {
                asset_url: url.clone(),
                label: "PDF".to_owned(),
                mime_type: Some("application/pdf".to_owned()),
                role: SourceAssetRole::Pdf,
            })
            .collect();

        records.push(SourceRecord {
            id: format!("{CIA_SOURCE}:{source_id}"),
            document_key: document_key(CIA_SOURCE, &source_id),
            source: CIA_SOURCE,
            source_id,
            title,
            date: None,
            collection: None,
            record_group: None,
            description: Some(description),
            origin_url: url.clone(),
            document_url: url,
            pdf_url,
            metadata: BTreeMap::new(),
            attachments,
            text_preview: None,
            citation_note: Some(CITATION_NOTE.to_owned()),
            terms_note: None,
        });
    }

    let has_next = has_next_link(html);
    let has_records = !records.is_empty();
    SearchPage {
        query: query.to_owned(),
        source: CIA_SEARCH_SOURCE,
        records,
        next_cursor: has_next.then(|| make_cursor(page + 1)),
        warnings: if has_next || has_records {
            Vec::new()
        } else {
            vec![SOURCE_CHANGED_WARNING.to_owned()]
        },
    }
}

pub fn parse_cia_document(html: &str, base_url: &str, fallback_url: &str) -> SourceRecord {
    let canonical = first_link_rel_href(html, "canonical");
    let url = canonical
        .as_deref()
        .map(|href| absolutize(href, base_url))
        .unwrap_or_else(|| fallback_url.to_owned());
    let source_id = cia_document_id_from_url(&url).unwrap_or_else(|| fallback_url.to_owned());
    let title = first_non_empty(&[
        first_tag_text(html, "h1"),
        first_tag_text(html, "title"),
        source_id.clone(),
    ]);
    let metadata = parse_metadata(html);
    let attachments = parse_attachments(html, base_url);
    let pdf_url = attachments
        .iter()
        .find(|asset| asset.role == SourceAssetRole::Pdf)
        .map(|asset| asset.asset_url.clone());
    let body_text = first_non_empty(&[
        first_tag_text(html, "main"),
        first_tag_text(html, "article"),
        first_class_text(html, "region-content"),
        first_tag_text(html, "body"),
    ]);

    SourceRecord {
        id: format!("{CIA_SOURCE}:{source_id}"),
        document_key: document_key(CIA_SOURCE, &source_id),
        source: CIA_SOURCE,
        source_id,
        title,
        date: None,
        collection: None,
        record_group: None,
        description: None,
        origin_url: url.clone(),
        document_url: url,
        pdf_url,
        metadata,
        attachments,
        text_preview: Some(truncate_chars(&body_text, 2_000)),
        citation_note: Some(CITATION_NOTE.to_owned()),
        terms_note: None,
    }
}

pub fn parse_cursor(cursor: Option<&str>) -> usize {
    cursor
        .and_then(|cursor| cursor.strip_prefix("cia-page-"))
        .and_then(|page| page.parse::<usize>().ok())
        .unwrap_or(0)
}

pub fn make_cursor(page: usize) -> String {
    format!("cia-page-{page}")
}

fn search_candidate_blocks(html: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    for class_name in ["search-result", "views-row"] {
        blocks.extend(elements_with_class(html, class_name));
    }
    blocks.extend(elements_by_tag(html, "article"));
    dedupe_preserve_order(blocks)
}

fn parse_metadata(html: &str) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();

    for block in elements_with_class(html, "field")
        .into_iter()
        .chain(elements_with_class(html, "document-meta"))
        .chain(elements_with_class(html, "metadata"))
        .chain(elements_by_tag(html, "dl"))
    {
        let label = first_non_empty(&[
            first_class_text(&block, "field-label"),
            first_tag_text(&block, "dt"),
            first_tag_text(&block, "label"),
            first_tag_text(&block, "strong"),
        ])
        .trim_end_matches(':')
        .to_owned();
        let value = first_non_empty(&[
            first_class_text(&block, "field-item"),
            first_tag_text(&block, "dd"),
            clean_html_text(&block)
                .replacen(&label, "", 1)
                .trim()
                .to_owned(),
        ]);
        if !label.is_empty() && !value.is_empty() && label.len() < 80 {
            metadata.insert(label, value);
        }
    }

    metadata
}

fn parse_attachments(html: &str, base_url: &str) -> Vec<SourceAsset> {
    let mut seen = HashSet::new();
    let mut assets = Vec::new();
    for (href, text) in anchors(html) {
        let normalized_href = href.to_ascii_lowercase();
        if !normalized_href.contains(".pdf") && !normalized_href.contains("/docs/") {
            continue;
        }
        let asset_url = absolutize(&href, base_url);
        if !seen.insert(asset_url.clone()) {
            continue;
        }
        let role = if normalized_href.contains(".pdf") {
            SourceAssetRole::Pdf
        } else {
            SourceAssetRole::Other
        };
        let mime_type = (role == SourceAssetRole::Pdf).then(|| "application/pdf".to_owned());
        let label = first_non_empty(&[text, "document".to_owned()]);
        assets.push(SourceAsset {
            asset_url,
            label,
            mime_type,
            role,
        });
    }
    assets
}

fn elements_with_class(html: &str, class_name: &str) -> Vec<String> {
    let mut elements = Vec::new();
    let mut cursor = 0;
    while let Some(start) = html[cursor..].find('<').map(|index| cursor + index) {
        let Some(open_end) = html[start..].find('>').map(|index| start + index) else {
            break;
        };
        let open = &html[start..=open_end];
        if tag_name(open).is_some()
            && attr_value(open, "class")
                .map(|classes| classes.split_whitespace().any(|class| class == class_name))
                .unwrap_or(false)
        {
            if let Some(element) = element_from_open_tag(html, start, open) {
                elements.push(element);
            }
        }
        cursor = open_end + 1;
    }
    elements
}

fn elements_by_tag(html: &str, wanted_tag: &str) -> Vec<String> {
    let mut elements = Vec::new();
    let mut cursor = 0;
    let wanted = wanted_tag.to_ascii_lowercase();
    while let Some(start) = html[cursor..].find('<').map(|index| cursor + index) {
        let Some(open_end) = html[start..].find('>').map(|index| start + index) else {
            break;
        };
        let open = &html[start..=open_end];
        if tag_name(open).as_deref() == Some(wanted.as_str()) {
            if let Some(element) = element_from_open_tag(html, start, open) {
                elements.push(element);
            }
        }
        cursor = open_end + 1;
    }
    elements
}

fn first_tag_text(html: &str, tag: &str) -> String {
    elements_by_tag(html, tag)
        .into_iter()
        .next()
        .map(|element| clean_html_text(&element))
        .unwrap_or_default()
}

fn first_class_text(html: &str, class_name: &str) -> String {
    elements_with_class(html, class_name)
        .into_iter()
        .next()
        .map(|element| clean_html_text(&element))
        .unwrap_or_default()
}

fn element_from_open_tag(html: &str, start: usize, open: &str) -> Option<String> {
    let tag = tag_name(open)?;
    if open.trim_end().ends_with("/>") {
        return Some(open.to_owned());
    }

    let lower = html.to_ascii_lowercase();
    let mut cursor = start + open.len();
    let mut depth = 1_usize;
    while depth > 0 {
        let next_open = lower[cursor..]
            .find(&format!("<{tag}"))
            .map(|index| cursor + index);
        let next_close = lower[cursor..]
            .find(&format!("</{tag}>"))
            .map(|index| cursor + index);

        match (next_open, next_close) {
            (Some(open_index), Some(close_index)) if open_index < close_index => {
                let open_end = lower[open_index..]
                    .find('>')
                    .map(|index| open_index + index)?;
                if !lower[open_index..=open_end].trim_end().ends_with("/>") {
                    depth += 1;
                }
                cursor = open_end + 1;
            }
            (_, Some(close_index)) => {
                depth -= 1;
                cursor = close_index + tag.len() + 3;
            }
            _ => return None,
        }
    }

    Some(html[start..cursor].to_owned())
}

fn tag_name(open_tag: &str) -> Option<String> {
    let trimmed = open_tag
        .trim_start_matches('<')
        .trim_start_matches('/')
        .trim_start();
    let name = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

fn anchors(html: &str) -> Vec<(String, String)> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while let Some(start) = html[cursor..].find("<a").map(|index| cursor + index) {
        let Some(open_end) = html[start..].find('>').map(|index| start + index) else {
            break;
        };
        let open = &html[start..=open_end];
        if let Some(href) = attr_value(open, "href") {
            let end = html[open_end + 1..]
                .to_ascii_lowercase()
                .find("</a>")
                .map(|index| open_end + 1 + index)
                .unwrap_or(open_end + 1);
            values.push((href, clean_html_text(&html[open_end + 1..end])));
        }
        cursor = open_end + 1;
    }
    values
}

fn first_href_matching(html: &str, predicate: impl Fn(&str) -> bool) -> Option<String> {
    anchors(html)
        .into_iter()
        .map(|(href, _text)| href)
        .find(|href| predicate(href))
}

fn anchor_text_for_href(html: &str, expected_href: &str) -> String {
    anchors(html)
        .into_iter()
        .find(|(href, _text)| href == expected_href)
        .map(|(_href, text)| text)
        .unwrap_or_default()
}

fn first_link_rel_href(html: &str, rel: &str) -> Option<String> {
    let mut cursor = 0;
    while let Some(start) = html[cursor..].find("<link").map(|index| cursor + index) {
        let Some(open_end) = html[start..].find('>').map(|index| start + index) else {
            break;
        };
        let open = &html[start..=open_end];
        let rel_matches = attr_value(open, "rel")
            .map(|value| value.eq_ignore_ascii_case(rel))
            .unwrap_or(false);
        if rel_matches {
            if let Some(href) = attr_value(open, "href") {
                return Some(href);
            }
        }
        cursor = open_end + 1;
    }
    None
}

fn has_next_link(html: &str) -> bool {
    anchors(html).into_iter().any(|(_href, text)| {
        text.eq_ignore_ascii_case("next") || text.to_ascii_lowercase().contains("next")
    }) || html.to_ascii_lowercase().contains("rel=\"next\"")
        || html.to_ascii_lowercase().contains("pager-next")
}

fn attr_value(open_tag: &str, attr: &str) -> Option<String> {
    let lower = open_tag.to_ascii_lowercase();
    let pattern = format!("{}=", attr.to_ascii_lowercase());
    let start = lower.find(&pattern)? + pattern.len();
    let quote = open_tag[start..].chars().next()?;
    if quote == '"' || quote == '\'' {
        let value_start = start + quote.len_utf8();
        let value_end = open_tag[value_start..].find(quote)? + value_start;
        return Some(open_tag[value_start..value_end].to_owned());
    }

    let value_end = open_tag[start..]
        .find(|ch: char| ch.is_whitespace() || ch == '>')
        .map(|index| start + index)
        .unwrap_or(open_tag.len());
    Some(open_tag[start..value_end].trim_end_matches('>').to_owned())
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
    decode_basic_entities(&clean_text(&text))
}

fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_basic_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn cia_document_id_from_url(url: &str) -> Option<String> {
    url.split("/document/")
        .nth(1)
        .and_then(|tail| tail.split(['?', '#']).next())
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
}

fn document_key(source: &str, source_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in source.bytes().chain([b':']).chain(source_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{source}-{hash:016x}")
}

fn dedupe_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}

fn absolutize(href: &str, base_url: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_owned();
    }

    let base = base_url.trim_end_matches('/');
    if href.starts_with("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return format!("{scheme}:{href}");
    }
    if href.starts_with('/') {
        let origin = base
            .split_once("://")
            .and_then(|(scheme, rest)| {
                rest.split('/')
                    .next()
                    .map(|host| format!("{scheme}://{host}"))
            })
            .unwrap_or_else(|| base.to_owned());
        return format!("{origin}{href}");
    }
    format!("{base}/{href}")
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_search_extracts_document_results() {
        let html = r#"
            <div class="search-result">
              <h3><a href="/readingroom/document/cia-rdp-test">Weather Modification</a></h3>
              <p>Released memo text.</p>
              <a href="/readingroom/docs/CIA-RDP-TEST.pdf">PDF</a>
            </div>"#;

        let parsed = parse_cia_search(html, "https://www.cia.gov", "weather modification", 0);

        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].source_id, "cia-rdp-test");
        assert_eq!(parsed.records[0].id, "cia:cia-rdp-test");
        assert_eq!(
            parsed.records[0].pdf_url.as_deref(),
            Some("https://www.cia.gov/readingroom/docs/CIA-RDP-TEST.pdf")
        );
        assert!(parsed.records[0].document_key.starts_with("cia-"));
        assert_ne!(parsed.records[0].document_key, parsed.records[0].source_id);
        assert!(parsed.next_cursor.is_none());
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn parse_search_uses_next_link_for_cursor() {
        let html = r#"
            <div class="search-result">
              <h3><a href="/readingroom/document/cia-rdp-test">Weather Modification</a></h3>
            </div>
            <nav><a rel="next" href="?page=1">Next</a></nav>"#;

        let parsed = parse_cia_search(html, "https://www.cia.gov", "weather modification", 0);

        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.next_cursor.as_deref(), Some("cia-page-1"));
    }

    #[test]
    fn parse_search_handles_nested_result_markup() {
        let html = r#"
            <div class="search-result">
              <div class="inner">
                <h3><a href="/readingroom/document/cia-rdp-nested">Nested Result</a></h3>
                <div><span>Released memo text.</span></div>
              </div>
            </div>"#;

        let parsed = parse_cia_search(html, "https://www.cia.gov", "nested", 0);

        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].source_id, "cia-rdp-nested");
        assert!(parsed.next_cursor.is_none());
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn parse_search_warns_when_shape_changes() {
        let parsed = parse_cia_search(
            "<html><body>No results</body></html>",
            DEFAULT_BASE_URL,
            "test",
            0,
        );

        assert!(parsed.records.is_empty());
        assert!(parsed.next_cursor.is_none());
        assert_eq!(parsed.warnings, vec![SOURCE_CHANGED_WARNING.to_owned()]);
    }

    #[test]
    fn parse_document_extracts_title_metadata_and_attachments() {
        let html = r#"
            <html>
              <head><link rel="canonical" href="/readingroom/document/cia-rdp-test"></head>
              <body>
                <main>
                  <h1>Climate Control</h1>
                  <div class="field"><span class="field-label">Document Type:</span><span class="field-item">CREST</span></div>
                  <a href="/readingroom/docs/CIA-RDP-TEST.pdf">Download</a>
                  <a href="/readingroom/docs/CIA-RDP-TEST.pdf">Duplicate</a>
                  <p>OCR preview text.</p>
                </main>
              </body>
            </html>"#;

        let doc = parse_cia_document(
            html,
            "https://www.cia.gov",
            "https://www.cia.gov/readingroom/document/cia-rdp-test",
        );

        assert_eq!(doc.source_id, "cia-rdp-test");
        assert_eq!(doc.id, "cia:cia-rdp-test");
        assert_eq!(doc.title, "Climate Control");
        assert_eq!(
            doc.pdf_url.as_deref(),
            Some("https://www.cia.gov/readingroom/docs/CIA-RDP-TEST.pdf")
        );
        assert_eq!(doc.attachments.len(), 1);
        assert_eq!(
            doc.metadata.get("Document Type").map(String::as_str),
            Some("CREST")
        );
        assert!(doc
            .text_preview
            .as_deref()
            .unwrap_or_default()
            .contains("OCR preview text."));
    }

    #[test]
    fn rejects_non_cia_document_urls() {
        let adapter = CiaAdapter::default();
        let err = adapter.document_url("https://www.cia.gov/readingroom/search/site/test");

        assert!(matches!(err, Err(SourceError::InvalidInput { .. })));
    }
}
