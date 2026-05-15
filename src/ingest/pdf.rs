use crate::ingest::chunk::PageText;
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedText {
    pub pages: Vec<PageText>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum TextExtraction {
    Io(std::io::Error),
    EmptyInput,
}

impl fmt::Display for TextExtraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "text extraction I/O error: {err}"),
            Self::EmptyInput => write!(f, "text extraction produced no pages"),
        }
    }
}

impl std::error::Error for TextExtraction {}

impl From<std::io::Error> for TextExtraction {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

pub trait TextExtractor {
    fn extract_pages(&self, path: &Path) -> Result<ExtractedText, TextExtraction>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TextFileExtractor;

impl TextExtractor for TextFileExtractor {
    fn extract_pages(&self, path: &Path) -> Result<ExtractedText, TextExtraction> {
        let text = fs::read_to_string(path)?;
        extracted_text_from_form_feed(&text)
    }
}

pub fn extracted_text_from_form_feed(text: &str) -> Result<ExtractedText, TextExtraction> {
    let normalized_pages = text
        .split('\x0C')
        .enumerate()
        .map(|(index, page)| PageText {
            page_number: (index + 1) as u32,
            text: normalize_page_text(page),
        })
        .collect::<Vec<_>>();

    let Some(first_text_page) = normalized_pages
        .iter()
        .position(|page| !page.text.is_empty())
    else {
        return Err(TextExtraction::EmptyInput);
    };
    let last_text_page = normalized_pages
        .iter()
        .rposition(|page| !page.text.is_empty())
        .unwrap_or(first_text_page);
    let pages = normalized_pages[first_text_page..=last_text_page].to_vec();

    Ok(ExtractedText {
        pages,
        warnings: Vec::new(),
    })
}

fn normalize_page_text(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}
