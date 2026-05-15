use crate::ingest::chunk::PageText;
use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedText {
    pub pages: Vec<PageText>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum TextExtraction {
    Io(std::io::Error),
    EmptyInput,
    UnavailableBinary {
        binary: PathBuf,
    },
    CommandFailed {
        binary: PathBuf,
        status: String,
        stderr: String,
    },
    Timeout {
        binary: PathBuf,
        timeout: Duration,
        stderr: String,
    },
    InvalidOutputPath {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for TextExtraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "text extraction I/O error: {err}"),
            Self::EmptyInput => write!(f, "text extraction produced no pages"),
            Self::UnavailableBinary { binary } => write!(
                f,
                "configured text extraction binary is unavailable: {}",
                binary.display()
            ),
            Self::CommandFailed {
                binary,
                status,
                stderr,
            } => write!(
                f,
                "text extraction command failed: {} exited with {status}: {stderr}",
                binary.display()
            ),
            Self::Timeout {
                binary,
                timeout,
                stderr,
            } => write!(
                f,
                "text extraction command timed out after {}s: {}: {stderr}",
                timeout.as_secs_f32(),
                binary.display()
            ),
            Self::InvalidOutputPath { path, reason } => {
                write!(
                    f,
                    "invalid text extraction output path {}: {reason}",
                    path.display()
                )
            }
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
        warnings: embedded_text_quality_warnings(&pages),
        pages,
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

fn embedded_text_quality_warnings(pages: &[PageText]) -> Vec<String> {
    let blank_pages = pages
        .iter()
        .filter(|page| page.text.trim().is_empty())
        .count();
    let non_blank_pages = pages.len().saturating_sub(blank_pages);
    let mut warnings = Vec::new();

    if blank_pages > 0 {
        warnings.push(format!(
            "embedded PDF text contains {blank_pages} blank page(s) among {} parsed page(s); OCR fallback may improve coverage",
            pages.len()
        ));
    }

    if non_blank_pages > 0 {
        let visible_chars = pages
            .iter()
            .filter(|page| !page.text.trim().is_empty())
            .map(|page| page.text.chars().filter(|ch| !ch.is_whitespace()).count())
            .sum::<usize>();
        let average_visible_chars = visible_chars / non_blank_pages;
        if average_visible_chars < 40 {
            warnings.push(format!(
                "embedded PDF text has low density ({average_visible_chars} visible chars/page); OCR fallback may improve quality"
            ));
        }
    }

    warnings
}
