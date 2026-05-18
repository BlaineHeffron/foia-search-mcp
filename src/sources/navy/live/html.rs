use scraper::{ElementRef, Html, Selector};

pub(crate) fn anchors(html: &str) -> Vec<(String, String)> {
    let document = Html::parse_document(html);
    let Ok(selector) = Selector::parse("a") else {
        return Vec::new();
    };

    document
        .select(&selector)
        .filter_map(|anchor| {
            let href = anchor.value().attr("href")?.trim();
            if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") {
                return None;
            }
            let text = anchor.text().collect::<Vec<_>>().join(" ");
            Some((href.to_owned(), normalize_space(&text)))
        })
        .collect()
}

pub(crate) fn table_rows(html: &str) -> Vec<Vec<CellLink>> {
    let document = Html::parse_document(html);
    let Ok(row_selector) = Selector::parse("tr") else {
        return Vec::new();
    };
    let Ok(cell_selector) = Selector::parse("td") else {
        return Vec::new();
    };

    document
        .select(&row_selector)
        .map(|row| {
            row.select(&cell_selector)
                .map(cell_from_element)
                .collect::<Vec<_>>()
        })
        .filter(|cells| !cells.is_empty())
        .collect()
}

pub(crate) fn clean_html_text(html: &str) -> String {
    let document = Html::parse_document(html);
    let text = document.root_element().text().collect::<Vec<_>>().join(" ");
    normalize_space(&text)
}

pub(crate) fn first_tag_text(html: &str, tag: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(tag).ok()?;
    document
        .select(&selector)
        .map(|node| normalize_space(&node.text().collect::<Vec<_>>().join(" ")))
        .find(|text| !text.is_empty())
}

pub(crate) fn normalize_space(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
pub(crate) struct CellLink {
    pub(crate) text: String,
    pub(crate) href: Option<String>,
}

fn cell_from_element(cell: ElementRef<'_>) -> CellLink {
    let text = normalize_space(&cell.text().collect::<Vec<_>>().join(" "));
    let href = Selector::parse("a")
        .ok()
        .and_then(|selector| {
            cell.select(&selector)
                .find_map(|anchor| anchor.value().attr("href").map(str::trim))
        })
        .filter(|href| !href.is_empty() && !href.starts_with('#'))
        .map(ToOwned::to_owned);

    CellLink { text, href }
}
