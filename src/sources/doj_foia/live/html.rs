pub(crate) fn first_tag_text(html: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let lower = html.to_ascii_lowercase();
    let start = match lower.find(&open) {
        Some(index) => index,
        None => return String::new(),
    };
    let open_end = match html[start..].find('>') {
        Some(index) => start + index,
        None => return String::new(),
    };
    let after = open_end + 1;
    let end = match lower[after..].find(&close) {
        Some(index) => after + index,
        None => return String::new(),
    };

    clean_html_text(&html[after..end])
}

pub(crate) fn anchors(html: &str) -> Vec<(String, String)> {
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

pub(crate) fn clean_html_text(value: &str) -> String {
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
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
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
