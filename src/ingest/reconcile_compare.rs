use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub(crate) struct RenderedPage<'a> {
    pub page_number: u32,
    pub text: &'a str,
}

pub(crate) fn file_matches_bytes(path: &Path, expected: &[u8]) -> io::Result<bool> {
    let mut comparator = FileComparator::open(path)?;
    if !comparator.consume(expected)? {
        return Ok(false);
    }
    comparator.finish()
}

pub(crate) fn file_matches_rendered_pages<'a>(
    path: &Path,
    pages: impl IntoIterator<Item = RenderedPage<'a>>,
) -> io::Result<bool> {
    let mut comparator = FileComparator::open(path)?;

    for (index, page) in pages.into_iter().enumerate() {
        if index > 0 && !comparator.consume(b"\n\n")? {
            return Ok(false);
        }
        if !comparator.consume(b"[page ")? {
            return Ok(false);
        }
        if !comparator.consume(page.page_number.to_string().as_bytes())? {
            return Ok(false);
        }
        if !comparator.consume(b"]\n")? {
            return Ok(false);
        }
        if !comparator.consume(page.text.as_bytes())? {
            return Ok(false);
        }
    }

    comparator.finish()
}

struct FileComparator {
    file: File,
}

impl FileComparator {
    fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
        })
    }

    fn consume(&mut self, mut expected: &[u8]) -> io::Result<bool> {
        let mut buffer = [0_u8; 8192];

        while !expected.is_empty() {
            let read_len = expected.len().min(buffer.len());
            let buffer = &mut buffer[..read_len];
            if let Err(error) = self.file.read_exact(buffer) {
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    return Ok(false);
                }
                return Err(error);
            }
            if buffer != &expected[..read_len] {
                return Ok(false);
            }
            expected = &expected[read_len..];
        }

        Ok(true)
    }

    fn finish(&mut self) -> io::Result<bool> {
        let mut trailing = [0_u8; 1];
        match self.file.read(&mut trailing) {
            Ok(0) => Ok(true),
            Ok(_) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reconcile_compare_matches_rendered_pages_without_full_file_read() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("document.txt");
        fs::write(&path, "[page 1]\nalpha\n\n[page 2]\nbravo").expect("write fixture");

        let matches = file_matches_rendered_pages(
            &path,
            [
                RenderedPage {
                    page_number: 1,
                    text: "alpha",
                },
                RenderedPage {
                    page_number: 2,
                    text: "bravo",
                },
            ],
        )
        .expect("compare rendered pages");

        assert!(matches);
    }

    #[test]
    fn reconcile_compare_detects_mismatch_and_trailing_content() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let mismatch = tempdir.path().join("mismatch.txt");
        let trailing = tempdir.path().join("trailing.txt");
        fs::write(&mismatch, "alpha").expect("write mismatch fixture");
        fs::write(&trailing, "alpha trailing").expect("write trailing fixture");

        assert!(!file_matches_bytes(&mismatch, b"bravo").expect("compare mismatch"));
        assert!(!file_matches_bytes(&trailing, b"alpha").expect("compare trailing"));
    }
}
