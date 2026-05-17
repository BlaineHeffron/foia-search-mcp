use scraper::{ElementRef, Selector};

use crate::sources::SourceError;

use super::NOAA_SOURCE;

pub(crate) fn selector(pattern: &str) -> Result<Selector, SourceError> {
    Selector::parse(pattern).map_err(|_| SourceError::SourceChanged {
        source: NOAA_SOURCE,
        message: "NOAA parser selector configuration is invalid.".to_owned(),
        url: None,
    })
}

pub(crate) fn first_text(scope: &ElementRef<'_>, selectors: &[&str]) -> Option<String> {
    selectors.iter().find_map(|pattern| {
        selector(pattern).ok().and_then(|sel| {
            scope
                .select(&sel)
                .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")))
                .find(|text| !text.is_empty())
        })
    })
}

pub(crate) fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn ensure_html_body(body: &str, url: &str) -> Result<(), SourceError> {
    let trimmed = body.trim_start();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Err(SourceError::SourceChanged {
            source: NOAA_SOURCE,
            message:
                "NOAA response returned JSON where HTML repository markup was expected for this endpoint."
                    .to_owned(),
            url: Some(url.to_owned()),
        });
    }

    if trimmed.starts_with("<?xml")
        || trimmed.starts_with("<record")
        || trimmed.starts_with("<OAI-PMH")
    {
        return Err(SourceError::SourceChanged {
            source: NOAA_SOURCE,
            message:
                "NOAA response returned XML where HTML repository markup was expected for this endpoint."
                    .to_owned(),
            url: Some(url.to_owned()),
        });
    }

    Ok(())
}
