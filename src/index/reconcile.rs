use crate::store::{SqliteStore, StoreError};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum FtsReconciliationIssueKind {
    Missing,
    Orphaned,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtsReconciliationIssue {
    pub issue: FtsReconciliationIssueKind,
    pub document_key: String,
    pub chunk_id: String,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtsReconciliationReport {
    pub canonical_chunk_count: usize,
    pub chunk_fts_row_count: usize,
    pub issues: Vec<FtsReconciliationIssue>,
}

#[derive(Debug)]
pub enum FtsReconciliationError {
    Store(StoreError),
}

impl fmt::Display for FtsReconciliationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FtsReconciliationError {}

impl From<StoreError> for FtsReconciliationError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<rusqlite::Error> for FtsReconciliationError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(StoreError::from(error))
    }
}

pub fn reconcile_sqlite_fts_index(
    store: &SqliteStore,
) -> Result<FtsReconciliationReport, FtsReconciliationError> {
    let canonical_rows = load_canonical_rows(store)?;
    let fts_rows = load_fts_rows(store)?;
    let canonical_chunk_count = canonical_rows.len();
    let chunk_fts_row_count = fts_rows.len();

    let mut canonical_by_identity = BTreeMap::new();
    for row in canonical_rows {
        let identity = ChunkIdentity::new(row.document_key.clone(), row.chunk_id.clone());
        canonical_by_identity.insert(identity, row);
    }

    let mut fts_by_identity: BTreeMap<ChunkIdentity, Vec<FtsRow>> = BTreeMap::new();
    for row in fts_rows {
        let identity = ChunkIdentity::new(row.document_key.clone(), row.chunk_id.clone());
        fts_by_identity.entry(identity).or_default().push(row);
    }

    let mut issues = Vec::new();

    for (identity, canonical) in &canonical_by_identity {
        let maybe_fts_rows = fts_by_identity.remove(identity);
        let Some(fts_rows_for_chunk) = maybe_fts_rows else {
            issues.push(FtsReconciliationIssue {
                issue: FtsReconciliationIssueKind::Missing,
                document_key: canonical.document_key.clone(),
                chunk_id: canonical.chunk_id.clone(),
                detail: "missing chunk_fts row for canonical chunk".to_owned(),
            });
            continue;
        };

        let matching_row_count = fts_rows_for_chunk
            .iter()
            .filter(|fts_row| fts_row.matches_canonical(canonical))
            .count();

        if matching_row_count == 1 && fts_rows_for_chunk.len() == 1 {
            continue;
        }

        if matching_row_count > 0 {
            let detail = match fts_rows_for_chunk
                .iter()
                .find(|fts_row| !fts_row.matches_canonical(canonical))
            {
                Some(observed) => {
                    let differing_fields = differing_fields(canonical, observed).join(", ");
                    format!(
                        "chunk_fts has {} rows for one canonical chunk (expected 1); first divergent row differs in [{differing_fields}]",
                        fts_rows_for_chunk.len()
                    )
                }
                None => format!(
                    "chunk_fts has {} matching rows for one canonical chunk (expected 1)",
                    fts_rows_for_chunk.len()
                ),
            };
            issues.push(FtsReconciliationIssue {
                issue: FtsReconciliationIssueKind::Stale,
                document_key: canonical.document_key.clone(),
                chunk_id: canonical.chunk_id.clone(),
                detail,
            });
        } else if let Some(observed) = fts_rows_for_chunk.first() {
            let differing_fields = differing_fields(canonical, observed).join(", ");
            let detail = format!(
                "chunk_fts row differs from canonical chunk fields [{differing_fields}] ({} observed row(s) for key)",
                fts_rows_for_chunk.len()
            );
            issues.push(FtsReconciliationIssue {
                issue: FtsReconciliationIssueKind::Stale,
                document_key: canonical.document_key.clone(),
                chunk_id: canonical.chunk_id.clone(),
                detail,
            });
        } else {
            issues.push(FtsReconciliationIssue {
                issue: FtsReconciliationIssueKind::Missing,
                document_key: canonical.document_key.clone(),
                chunk_id: canonical.chunk_id.clone(),
                detail: "missing chunk_fts row for canonical chunk".to_owned(),
            });
        }
    }

    for (identity, fts_rows_for_chunk) in fts_by_identity {
        for row in fts_rows_for_chunk {
            issues.push(FtsReconciliationIssue {
                issue: FtsReconciliationIssueKind::Orphaned,
                document_key: identity.document_key.clone(),
                chunk_id: identity.chunk_id.clone(),
                detail: format!(
                    "chunk_fts row has no matching canonical chunk (rowid={})",
                    row.rowid
                ),
            });
        }
    }

    issues.sort_by(|left, right| {
        left.document_key
            .cmp(&right.document_key)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
            .then_with(|| left.issue.cmp(&right.issue))
            .then_with(|| left.detail.cmp(&right.detail))
    });

    Ok(FtsReconciliationReport {
        canonical_chunk_count,
        chunk_fts_row_count,
        issues,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ChunkIdentity {
    document_key: String,
    chunk_id: String,
}

impl ChunkIdentity {
    fn new(document_key: String, chunk_id: String) -> Self {
        Self {
            document_key,
            chunk_id,
        }
    }
}

#[derive(Clone, Debug)]
struct CanonicalRow {
    document_key: String,
    chunk_id: String,
    source: String,
    title: String,
    body: String,
    page_start: i64,
    page_end: i64,
}

#[derive(Clone, Debug)]
struct FtsRow {
    rowid: i64,
    document_key: String,
    chunk_id: String,
    source: String,
    title: String,
    body: String,
    page_start: i64,
    page_end: i64,
}

impl FtsRow {
    fn matches_canonical(&self, canonical: &CanonicalRow) -> bool {
        self.document_key == canonical.document_key
            && self.chunk_id == canonical.chunk_id
            && self.source == canonical.source
            && self.title == canonical.title
            && self.body == canonical.body
            && self.page_start == canonical.page_start
            && self.page_end == canonical.page_end
    }
}

fn differing_fields(canonical: &CanonicalRow, observed: &FtsRow) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if canonical.source != observed.source {
        fields.push("source");
    }
    if canonical.title != observed.title {
        fields.push("title");
    }
    if canonical.body != observed.body {
        fields.push("body");
    }
    if canonical.page_start != observed.page_start {
        fields.push("page_start");
    }
    if canonical.page_end != observed.page_end {
        fields.push("page_end");
    }
    if fields.is_empty() {
        fields.push("duplicate_or_unexpected_row_shape");
    }
    fields
}

fn load_canonical_rows(store: &SqliteStore) -> Result<Vec<CanonicalRow>, FtsReconciliationError> {
    let mut statement = store.connection().prepare(
        "
        SELECT d.document_key, c.chunk_id, d.source, d.title, c.text, c.page_start, c.page_end
        FROM chunks c
        INNER JOIN documents d ON d.id = c.document_id
        ORDER BY d.document_key, c.chunk_id
        ",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(CanonicalRow {
            document_key: row.get(0)?,
            chunk_id: row.get(1)?,
            source: row.get(2)?,
            title: row.get(3)?,
            body: row.get(4)?,
            page_start: row.get(5)?,
            page_end: row.get(6)?,
        })
    })?;

    let mut canonical = Vec::new();
    for row in rows {
        canonical.push(row?);
    }
    Ok(canonical)
}

fn load_fts_rows(store: &SqliteStore) -> Result<Vec<FtsRow>, FtsReconciliationError> {
    let mut statement = store.connection().prepare(
        "
        SELECT rowid, document_key, chunk_id, source, title, body, page_start, page_end
        FROM chunk_fts
        ORDER BY document_key, chunk_id, rowid
        ",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(FtsRow {
            rowid: row.get(0)?,
            document_key: row.get(1)?,
            chunk_id: row.get(2)?,
            source: row.get(3)?,
            title: row.get(4)?,
            body: row.get(5)?,
            page_start: row.get(6)?,
            page_end: row.get(7)?,
        })
    })?;

    let mut observed = Vec::new();
    for row in rows {
        observed.push(row?);
    }
    Ok(observed)
}
