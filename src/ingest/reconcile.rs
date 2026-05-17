use super::reconcile_compare::{file_matches_bytes, file_matches_rendered_pages, RenderedPage};
use crate::store::{ContentAddressedStore, DocumentKey, SqliteStore, StoreError};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DerivedArtifactKind {
    DocumentText,
    PageText,
    OcrPageText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DerivedArtifactIssueKind {
    Missing,
    Stale,
    Orphaned,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DerivedArtifactIssue {
    pub kind: DerivedArtifactKind,
    pub issue: DerivedArtifactIssueKind,
    pub path: PathBuf,
    pub page_number: Option<u32>,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedArtifactReport {
    pub document_key: DocumentKey,
    pub issues: Vec<DerivedArtifactIssue>,
}

#[derive(Debug)]
pub enum ReconcileError {
    Store(StoreError),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "{operation} failed for {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ReconcileError {}

impl From<StoreError> for ReconcileError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

pub fn reconcile_derived_artifacts_for_document(
    store: &SqliteStore,
    files: &ContentAddressedStore,
    document_id_or_key: &str,
) -> Result<DerivedArtifactReport, ReconcileError> {
    let metadata = store.get_document_metadata(document_id_or_key)?;
    let expected_pages = load_document_pages(store, metadata.id)?;
    let document_key = metadata.document_key;

    let mut issues = Vec::new();

    let document_text_path = files.derived_document_text_path(&document_key);
    if expected_pages.is_empty() {
        if document_text_path.exists() {
            issues.push(orphaned_issue(
                DerivedArtifactKind::DocumentText,
                document_text_path,
                None,
                "document has no stored pages".to_owned(),
            ));
        }
    } else {
        push_expected_document_file_issues(
            &mut issues,
            DerivedArtifactKind::DocumentText,
            &document_text_path,
            None,
            &expected_pages,
        )?;
    }

    let mut expected_text_pages = BTreeSet::new();
    let mut expected_ocr_pages = BTreeSet::new();
    for page in &expected_pages {
        let page_path = files.derived_page_text_path(&document_key, page.page_number);
        push_expected_text_file_issues(
            &mut issues,
            DerivedArtifactKind::PageText,
            &page_path,
            Some(page.page_number),
            page.text.as_bytes(),
        )?;
        expected_text_pages.insert(page.page_number);
        if page.text_source == "local_ocr" {
            let ocr_path = derived_ocr_page_text_path(files, &document_key, page.page_number);
            push_expected_text_file_issues(
                &mut issues,
                DerivedArtifactKind::OcrPageText,
                &ocr_path,
                Some(page.page_number),
                page.text.as_bytes(),
            )?;
            expected_ocr_pages.insert(page.page_number);
        }
    }

    collect_orphaned_page_files(
        &mut issues,
        text_page_dir(files, &document_key),
        &expected_text_pages,
        DerivedArtifactKind::PageText,
    )?;
    collect_orphaned_page_files(
        &mut issues,
        ocr_page_dir(files, &document_key),
        &expected_ocr_pages,
        DerivedArtifactKind::OcrPageText,
    )?;

    issues.sort();

    Ok(DerivedArtifactReport {
        document_key,
        issues,
    })
}

#[derive(Clone, Debug)]
struct DocumentPage {
    page_number: u32,
    text: String,
    text_source: String,
}

fn load_document_pages(
    store: &SqliteStore,
    document_id: i64,
) -> Result<Vec<DocumentPage>, ReconcileError> {
    let mut stmt = store
        .connection()
        .prepare(
            "
            SELECT page_number, text, text_source
            FROM pages
            WHERE document_id = ?1
            ORDER BY page_number
            ",
        )
        .map_err(StoreError::from)
        .map_err(ReconcileError::from)?;

    let rows = stmt
        .query_map([document_id], |row| {
            let page_number: i64 = row.get(0)?;
            Ok(DocumentPage {
                page_number: page_number.max(0) as u32,
                text: row.get(1)?,
                text_source: row.get(2)?,
            })
        })
        .map_err(StoreError::from)
        .map_err(ReconcileError::from)?;

    let mut pages = Vec::new();
    for row in rows {
        pages.push(
            row.map_err(StoreError::from)
                .map_err(ReconcileError::from)?,
        );
    }

    Ok(pages)
}

fn push_expected_text_file_issues(
    issues: &mut Vec<DerivedArtifactIssue>,
    kind: DerivedArtifactKind,
    path: &Path,
    page_number: Option<u32>,
    expected: &[u8],
) -> Result<(), ReconcileError> {
    push_expected_file_issues(issues, kind, path, page_number, |path| {
        file_matches_bytes(path, expected)
    })
}

fn push_expected_document_file_issues(
    issues: &mut Vec<DerivedArtifactIssue>,
    kind: DerivedArtifactKind,
    path: &Path,
    page_number: Option<u32>,
    expected_pages: &[DocumentPage],
) -> Result<(), ReconcileError> {
    push_expected_file_issues(issues, kind, path, page_number, |path| {
        file_matches_rendered_pages(
            path,
            expected_pages.iter().map(|page| RenderedPage {
                page_number: page.page_number,
                text: &page.text,
            }),
        )
    })
}

fn push_expected_file_issues(
    issues: &mut Vec<DerivedArtifactIssue>,
    kind: DerivedArtifactKind,
    path: &Path,
    page_number: Option<u32>,
    matches_expected: impl FnOnce(&Path) -> std::io::Result<bool>,
) -> Result<(), ReconcileError> {
    if !path.exists() {
        issues.push(missing_issue(
            kind,
            path.to_path_buf(),
            page_number,
            "expected derived artifact is absent".to_owned(),
        ));
        return Ok(());
    }

    if !path.is_file() {
        issues.push(stale_issue(
            kind,
            path.to_path_buf(),
            page_number,
            "derived artifact path is not a regular file".to_owned(),
        ));
        return Ok(());
    }

    let matches_expected = matches_expected(path).map_err(|source| ReconcileError::Io {
        operation: "compare_text_file",
        path: path.to_path_buf(),
        source,
    })?;
    if !matches_expected {
        issues.push(stale_issue(
            kind,
            path.to_path_buf(),
            page_number,
            "derived artifact content differs from SQLite page state".to_owned(),
        ));
    }

    Ok(())
}

fn collect_orphaned_page_files(
    issues: &mut Vec<DerivedArtifactIssue>,
    dir: PathBuf,
    expected_pages: &BTreeSet<u32>,
    kind: DerivedArtifactKind,
) -> Result<(), ReconcileError> {
    if !dir.exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Err(ReconcileError::Io {
            operation: "read_dir",
            path: dir,
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "expected derived page artifact directory",
            ),
        });
    }

    for entry in fs::read_dir(&dir).map_err(|source| ReconcileError::Io {
        operation: "read_dir",
        path: dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| ReconcileError::Io {
            operation: "read_dir_entry",
            path: dir.clone(),
            source,
        })?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let page_number = parse_page_number(file_name);

        if !path.is_file() {
            if page_number.is_some_and(|value| expected_pages.contains(&value)) {
                // Expected page paths are handled by stale checks above; only report
                // unexpected non-file entries as orphaned/manual-review artifacts.
                continue;
            }

            issues.push(orphaned_issue(
                kind,
                path,
                page_number,
                "derived page artifact entry is not a regular file".to_owned(),
            ));
            continue;
        }

        let Some(page_number) = page_number else {
            issues.push(orphaned_issue(
                kind,
                path,
                None,
                "derived page artifact filename is not '<page>.txt'".to_owned(),
            ));
            continue;
        };

        if !expected_pages.contains(&page_number) {
            issues.push(orphaned_issue(
                kind,
                path,
                Some(page_number),
                "no matching SQLite page state for derived artifact".to_owned(),
            ));
        }
    }

    Ok(())
}

fn parse_page_number(file_name: &str) -> Option<u32> {
    let stem = file_name.strip_suffix(".txt")?;
    stem.parse::<u32>().ok().filter(|value| *value > 0)
}

fn text_page_dir(files: &ContentAddressedStore, document_key: &DocumentKey) -> PathBuf {
    files
        .root()
        .join("text")
        .join("pages")
        .join(document_key.as_str())
}

fn ocr_page_dir(files: &ContentAddressedStore, document_key: &DocumentKey) -> PathBuf {
    files
        .root()
        .join("ocr")
        .join("pages")
        .join(document_key.as_str())
}

fn derived_ocr_page_text_path(
    files: &ContentAddressedStore,
    document_key: &DocumentKey,
    page_number: u32,
) -> PathBuf {
    ocr_page_dir(files, document_key).join(format!("{page_number}.txt"))
}

fn missing_issue(
    kind: DerivedArtifactKind,
    path: PathBuf,
    page_number: Option<u32>,
    detail: String,
) -> DerivedArtifactIssue {
    DerivedArtifactIssue {
        kind,
        issue: DerivedArtifactIssueKind::Missing,
        path,
        page_number,
        detail,
    }
}

fn stale_issue(
    kind: DerivedArtifactKind,
    path: PathBuf,
    page_number: Option<u32>,
    detail: String,
) -> DerivedArtifactIssue {
    DerivedArtifactIssue {
        kind,
        issue: DerivedArtifactIssueKind::Stale,
        path,
        page_number,
        detail,
    }
}

fn orphaned_issue(
    kind: DerivedArtifactKind,
    path: PathBuf,
    page_number: Option<u32>,
    detail: String,
) -> DerivedArtifactIssue {
    DerivedArtifactIssue {
        kind,
        issue: DerivedArtifactIssueKind::Orphaned,
        path,
        page_number,
        detail,
    }
}
