use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageText {
    pub page_number: u32,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkOptions {
    pub target_tokens: usize,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self { target_tokens: 800 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chunk {
    pub chunk_id: String,
    pub page_start: u32,
    pub page_end: u32,
    pub text: String,
    pub token_estimate: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChunkError {
    EmptyPages,
    InvalidPageNumber(u32),
}

impl fmt::Display for ChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPages => write!(f, "cannot chunk an empty page list"),
            Self::InvalidPageNumber(page) => write!(f, "invalid page number: {page}"),
        }
    }
}

impl std::error::Error for ChunkError {}

pub fn chunk_pages(pages: &[PageText], options: &ChunkOptions) -> Result<Vec<Chunk>, ChunkError> {
    if pages.is_empty() {
        return Err(ChunkError::EmptyPages);
    }

    let target_tokens = options.target_tokens.max(1);
    let mut chunks = Vec::new();
    let mut current_pages = Vec::new();
    let mut current_tokens = 0_usize;

    for page in pages {
        if page.page_number == 0 {
            return Err(ChunkError::InvalidPageNumber(page.page_number));
        }

        let page_tokens = estimate_tokens(&page.text);
        if !current_pages.is_empty() && current_tokens + page_tokens > target_tokens {
            chunks.push(build_chunk(chunks.len() + 1, &current_pages));
            current_pages.clear();
            current_tokens = 0;
        }

        current_tokens += page_tokens;
        current_pages.push(page.clone());
    }

    if !current_pages.is_empty() {
        chunks.push(build_chunk(chunks.len() + 1, &current_pages));
    }

    Ok(chunks)
}

fn build_chunk(sequence: usize, pages: &[PageText]) -> Chunk {
    let page_start = pages.first().map(|page| page.page_number).unwrap_or(1);
    let page_end = pages
        .last()
        .map(|page| page.page_number)
        .unwrap_or(page_start);
    let text = pages
        .iter()
        .map(|page| page.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let token_estimate = estimate_tokens(&text);

    Chunk {
        chunk_id: format!("chunk-{sequence:04}"),
        page_start,
        page_end,
        text,
        token_estimate,
    }
}

fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunking_preserves_page_ranges() {
        let chunks = chunk_pages(
            &[
                PageText {
                    page_number: 1,
                    text: "alpha beta".to_owned(),
                },
                PageText {
                    page_number: 2,
                    text: "gamma delta".to_owned(),
                },
                PageText {
                    page_number: 3,
                    text: "epsilon zeta".to_owned(),
                },
            ],
            &ChunkOptions { target_tokens: 4 },
        )
        .expect("chunk pages");

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].page_start, 1);
        assert_eq!(chunks[0].page_end, 2);
        assert_eq!(chunks[1].page_start, 3);
        assert_eq!(chunks[1].page_end, 3);
    }
}
