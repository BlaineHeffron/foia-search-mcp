use crate::ingest::reconcile::DerivedArtifactKind;
use crate::ingest::reconcile_repair::{
    DerivedArtifactRepairAction, DerivedArtifactRepairPlan, DerivedArtifactRewriteReason,
};
use crate::store::{ContentAddressedStore, DocumentKey, SqliteStore, StoreError, StoredPageText};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static REPAIR_TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
const LOCAL_OCR_TEXT_SOURCE: &str = "local_ocr";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DerivedArtifactApplyReport {
    pub rewritten: usize,
    pub already_current: usize,
    pub skipped_manual_review: usize,
}

#[derive(Debug)]
pub enum RepairApplyError {
    Store(StoreError),
    InvalidAction {
        detail: String,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for RepairApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::InvalidAction { detail } => write!(f, "invalid repair action: {detail}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "{operation} failed for {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for RepairApplyError {}

impl From<StoreError> for RepairApplyError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

pub fn apply_derived_artifact_repairs(
    store: &SqliteStore,
    files: &ContentAddressedStore,
    plan: &DerivedArtifactRepairPlan,
) -> Result<DerivedArtifactApplyReport, RepairApplyError> {
    let metadata = store.get_document_metadata(plan.document_key.as_str())?;
    if metadata.document_key != plan.document_key {
        return Err(RepairApplyError::InvalidAction {
            detail: format!(
                "plan document_key '{}' does not match stored key '{}'",
                plan.document_key, metadata.document_key
            ),
        });
    }

    let pages = load_document_pages(store, metadata.id)?;
    let mut report = DerivedArtifactApplyReport::default();

    for action in &plan.actions {
        match action {
            DerivedArtifactRepairAction::ManualReview { .. } => {
                report.skipped_manual_review += 1;
            }
            DerivedArtifactRepairAction::RewriteFromSqlite {
                kind,
                path,
                page_number,
                reason,
            } => {
                let expected_path =
                    expected_path_for_action(files, &plan.document_key, *kind, *page_number)?;
                if path != &expected_path {
                    return Err(RepairApplyError::InvalidAction {
                        detail: format!(
                            "action path '{}' does not match canonical path '{}'",
                            path.display(),
                            expected_path.display()
                        ),
                    });
                }

                let expected_bytes = expected_bytes_for_action(*kind, *page_number, &pages)?;
                if path.is_file() {
                    let matches =
                        super::reconcile_compare::file_matches_bytes(path, &expected_bytes)
                            .map_err(|source| RepairApplyError::Io {
                                operation: "compare_text_file",
                                path: path.clone(),
                                source,
                            })?;
                    if matches {
                        report.already_current += 1;
                        continue;
                    }
                } else if path.exists() {
                    return Err(RepairApplyError::InvalidAction {
                        detail: format!("target path '{}' is not a regular file", path.display()),
                    });
                }

                write_file_atomic(path, &expected_bytes, *reason)?;
                report.rewritten += 1;
            }
        }
    }

    Ok(report)
}

fn expected_path_for_action(
    files: &ContentAddressedStore,
    document_key: &DocumentKey,
    kind: DerivedArtifactKind,
    page_number: Option<u32>,
) -> Result<PathBuf, RepairApplyError> {
    match kind {
        DerivedArtifactKind::DocumentText => {
            if page_number.is_some() {
                return Err(RepairApplyError::InvalidAction {
                    detail: "document text rewrite must not include page_number".to_owned(),
                });
            }
            Ok(files.derived_document_text_path(document_key))
        }
        DerivedArtifactKind::PageText => {
            let page_number = validate_page_number(page_number, kind)?;
            Ok(files.derived_page_text_path(document_key, page_number))
        }
        DerivedArtifactKind::OcrPageText => {
            let page_number = validate_page_number(page_number, kind)?;
            Ok(files
                .root()
                .join("ocr")
                .join("pages")
                .join(document_key.as_str())
                .join(format!("{page_number}.txt")))
        }
    }
}

fn validate_page_number(
    page_number: Option<u32>,
    kind: DerivedArtifactKind,
) -> Result<u32, RepairApplyError> {
    match page_number {
        Some(value) if value > 0 => Ok(value),
        _ => Err(RepairApplyError::InvalidAction {
            detail: format!("{kind:?} rewrite requires a positive page_number"),
        }),
    }
}

fn expected_bytes_for_action(
    kind: DerivedArtifactKind,
    page_number: Option<u32>,
    pages: &[StoredPageText],
) -> Result<Vec<u8>, RepairApplyError> {
    match kind {
        DerivedArtifactKind::DocumentText => render_document_text(pages),
        DerivedArtifactKind::PageText => {
            let page_number = validate_page_number(page_number, kind)?;
            let page = page_by_number(pages, page_number)?;
            Ok(page.text.as_bytes().to_vec())
        }
        DerivedArtifactKind::OcrPageText => {
            let page_number = validate_page_number(page_number, kind)?;
            let page = page_by_number(pages, page_number)?;
            if page.text_source != LOCAL_OCR_TEXT_SOURCE {
                return Err(RepairApplyError::InvalidAction {
                    detail: format!(
                        "page {} text_source '{}' is not '{}' for OcrPageText rewrite",
                        page_number, page.text_source, LOCAL_OCR_TEXT_SOURCE
                    ),
                });
            }
            Ok(page.text.as_bytes().to_vec())
        }
    }
}

fn render_document_text(pages: &[StoredPageText]) -> Result<Vec<u8>, RepairApplyError> {
    if pages.is_empty() {
        return Err(RepairApplyError::InvalidAction {
            detail: "document text rewrite requires at least one stored page".to_owned(),
        });
    }

    let mut rendered = String::new();
    for (index, page) in pages.iter().enumerate() {
        if index > 0 {
            rendered.push_str("\n\n");
        }
        rendered.push_str("[page ");
        rendered.push_str(&page.page_number.to_string());
        rendered.push_str("]\n");
        rendered.push_str(&page.text);
    }

    Ok(rendered.into_bytes())
}

fn page_by_number(
    pages: &[StoredPageText],
    page_number: u32,
) -> Result<&StoredPageText, RepairApplyError> {
    pages
        .iter()
        .find(|page| page.page_number == page_number)
        .ok_or_else(|| RepairApplyError::InvalidAction {
            detail: format!("no stored page {page_number} exists in SQLite"),
        })
}

fn write_file_atomic(
    target_path: &Path,
    bytes: &[u8],
    reason: DerivedArtifactRewriteReason,
) -> Result<(), RepairApplyError> {
    let parent = target_path
        .parent()
        .ok_or_else(|| RepairApplyError::InvalidAction {
            detail: format!(
                "target path '{}' has no parent directory",
                target_path.display()
            ),
        })?;

    fs::create_dir_all(parent).map_err(|source| RepairApplyError::Io {
        operation: "create_dir_all",
        path: parent.to_path_buf(),
        source,
    })?;

    let temp_path = parent.join(unique_temp_name(reason));
    let write_result = write_temp_file(&temp_path, bytes)
        .and_then(|_| replace_target_file(&temp_path, target_path));

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(())
}

fn write_temp_file(temp_path: &Path, bytes: &[u8]) -> Result<(), RepairApplyError> {
    let mut file = File::create(temp_path).map_err(|source| RepairApplyError::Io {
        operation: "create_temp_file",
        path: temp_path.to_path_buf(),
        source,
    })?;
    file.write_all(bytes)
        .map_err(|source| RepairApplyError::Io {
            operation: "write_temp_file",
            path: temp_path.to_path_buf(),
            source,
        })?;
    file.sync_all().map_err(|source| RepairApplyError::Io {
        operation: "sync_temp_file",
        path: temp_path.to_path_buf(),
        source,
    })
}

fn replace_target_file(temp_path: &Path, target_path: &Path) -> Result<(), RepairApplyError> {
    if target_path.exists() {
        fs::remove_file(target_path).map_err(|source| RepairApplyError::Io {
            operation: "remove_stale_target_file",
            path: target_path.to_path_buf(),
            source,
        })?;
    }
    fs::rename(temp_path, target_path).map_err(|source| RepairApplyError::Io {
        operation: "rename_temp_file",
        path: target_path.to_path_buf(),
        source,
    })
}

fn unique_temp_name(reason: DerivedArtifactRewriteReason) -> String {
    let sequence = REPAIR_TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let reason = match reason {
        DerivedArtifactRewriteReason::Missing => "missing",
        DerivedArtifactRewriteReason::Stale => "stale",
    };
    format!(
        ".repair-apply-{}-{}-{sequence}.tmp",
        std::process::id(),
        reason
    )
}

fn load_document_pages(
    store: &SqliteStore,
    document_id: i64,
) -> Result<Vec<StoredPageText>, RepairApplyError> {
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
        .map_err(RepairApplyError::from)?;

    let rows = stmt
        .query_map([document_id], |row| {
            let page_number: i64 = row.get(0)?;
            Ok(StoredPageText {
                page_number: page_number.max(0) as u32,
                text: row.get(1)?,
                text_source: row.get(2)?,
            })
        })
        .map_err(StoreError::from)
        .map_err(RepairApplyError::from)?;

    let mut pages = Vec::new();
    for row in rows {
        pages.push(
            row.map_err(StoreError::from)
                .map_err(RepairApplyError::from)?,
        );
    }

    Ok(pages)
}
