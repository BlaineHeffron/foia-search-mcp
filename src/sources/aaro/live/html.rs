pub(crate) fn anchors(html: &str) -> Vec<(String, String)> {
    let mut links = Vec::new();
    let mut cursor = html;

    while let Some(anchor_start) = find_case_insensitive(cursor, "<a") {
        let after_start = &cursor[anchor_start..];
        let Some(tag_end_rel) = after_start.find('>') else {
            break;
        };
        let tag = &after_start[..=tag_end_rel];
        let Some(href) = attr_value(tag, "href") else {
            cursor = &after_start[tag_end_rel + 1..];
            continue;
        };

        let content = &after_start[tag_end_rel + 1..];
        let Some(close_rel) = find_case_insensitive(content, "</a>") else {
            break;
        };
        let label = strip_tags(&content[..close_rel]);
        if !href.trim().is_empty() {
            links.push((href.trim().to_owned(), label));
        }

        cursor = &content[close_rel + "</a>".len()..];
    }

    links
}

pub(crate) fn first_tag_text(html: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let Some(open_index) = find_case_insensitive(html, &open) else {
        return String::new();
    };
    let tail = &html[open_index..];
    let Some(open_end_rel) = tail.find('>') else {
        return String::new();
    };
    let content = &tail[open_end_rel + 1..];
    let Some(close_rel) = find_case_insensitive(content, &close) else {
        return String::new();
    };

    strip_tags(&content[..close_rel])
}

pub(crate) fn clean_html_text(html: &str) -> String {
    strip_tags(html)
}

pub(crate) fn meta_content(html: &str, name: &str) -> Option<String> {
    let mut cursor = html;
    while let Some(meta_start) = find_case_insensitive(cursor, "<meta") {
        let tail = &cursor[meta_start..];
        let Some(tag_end_rel) = tail.find('>') else {
            break;
        };
        let tag = &tail[..=tag_end_rel];

        let matches_name = attr_value(tag, "name")
            .map(|value| value.eq_ignore_ascii_case(name))
            .unwrap_or(false);
        if matches_name {
            let content = attr_value(tag, "content")?;
            if !content.trim().is_empty() {
                return Some(content.trim().to_owned());
            }
        }

        cursor = &tail[tag_end_rel + 1..];
    }
    None
}

fn attr_value<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{attr}=");
    let attr_index = lower.find(&needle)? + needle.len();
    let remainder = &tag[attr_index..];
    let mut chars = remainder.chars();
    let quote = chars.next()?;

    if quote == '"' || quote == '\'' {
        let close = remainder[1..].find(quote)? + 1;
        Some(&remainder[1..close])
    } else {
        let end = remainder
            .find([' ', '>', '\t', '\n', '\r'])
            .unwrap_or(remainder.len());
        Some(&remainder[..end])
    }
}

fn strip_tags(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_tag = false;

    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}
