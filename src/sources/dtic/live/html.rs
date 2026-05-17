use scraper::{Html, Selector};

use crate::sources::SourceError;

use super::DTIC_SOURCE;

pub(crate) fn selector(pattern: &str) -> Result<Selector, SourceError> {
    Selector::parse(pattern).map_err(|_| SourceError::SourceChanged {
        source: DTIC_SOURCE,
        message: "DTIC parser selector configuration is invalid.".to_owned(),
        url: None,
    })
}

pub(crate) fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn first_text_from_document(document: &Html, selectors: &[&str]) -> Option<String> {
    selectors.iter().find_map(|pattern| {
        selector(pattern).ok().and_then(|sel| {
            document
                .select(&sel)
                .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")))
                .find(|value| !value.is_empty())
        })
    })
}

pub(crate) fn meta_content(document: &Html, selectors: &[&str]) -> Option<String> {
    selectors.iter().find_map(|pattern| {
        selector(pattern).ok().and_then(|sel| {
            document
                .select(&sel)
                .find_map(|node| node.value().attr("content"))
                .map(clean_text)
                .filter(|value| !value.is_empty())
        })
    })
}

pub(crate) fn ensure_html_body(body: &str, url: &str) -> Result<(), SourceError> {
    let trimmed = body.trim_start();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Err(SourceError::SourceChanged {
            source: DTIC_SOURCE,
            message: "DTIC response returned JSON where HTML citation markup was expected."
                .to_owned(),
            url: Some(url.to_owned()),
        });
    }

    if trimmed.starts_with("<?xml") || trimmed.starts_with("<urlset") || trimmed.starts_with("<rss")
    {
        return Err(SourceError::SourceChanged {
            source: DTIC_SOURCE,
            message: "DTIC response returned XML where HTML citation markup was expected."
                .to_owned(),
            url: Some(url.to_owned()),
        });
    }

    Ok(())
}
