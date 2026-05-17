use crate::index::reconcile::{
    FtsReconciliationError, FtsReconciliationIssueKind, FtsReconciliationReport,
};
use crate::store::SqliteStore;
use rusqlite::{params, Transaction};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FtsRepairActionKind {
    RewriteFromCanonical,
    ManualReviewOrphan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtsRepairAction {
    pub action: FtsRepairActionKind,
    pub document_key: String,
    pub chunk_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtsRepairPlan {
    pub actions: Vec<FtsRepairAction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtsRepairApplyResult {
    pub rewritten_count: usize,
    pub skipped_count: usize,
    pub manual_review_count: usize,
}

pub fn plan_sqlite_fts_repairs(report: &FtsReconciliationReport) -> FtsRepairPlan {
    let mut planned_rewrites = BTreeSet::new();
    let mut actions = Vec::new();

    for issue in &report.issues {
        match issue.issue {
            FtsReconciliationIssueKind::Missing | FtsReconciliationIssueKind::Stale => {
                let identity = (issue.document_key.clone(), issue.chunk_id.clone());
                if planned_rewrites.insert(identity) {
                    actions.push(FtsRepairAction {
                        action: FtsRepairActionKind::RewriteFromCanonical,
                        document_key: issue.document_key.clone(),
                        chunk_id: issue.chunk_id.clone(),
                        reason: issue.detail.clone(),
                    });
                }
            }
            FtsReconciliationIssueKind::Orphaned => {
                actions.push(FtsRepairAction {
                    action: FtsRepairActionKind::ManualReviewOrphan,
                    document_key: issue.document_key.clone(),
                    chunk_id: issue.chunk_id.clone(),
                    reason: issue.detail.clone(),
                });
            }
        }
    }

    actions.sort_by(|left, right| {
        left.document_key
            .cmp(&right.document_key)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
            .then_with(|| action_sort_key(left.action).cmp(&action_sort_key(right.action)))
            .then_with(|| left.reason.cmp(&right.reason))
    });

    FtsRepairPlan { actions }
}

pub fn apply_sqlite_fts_repair_plan(
    store: &SqliteStore,
    plan: &FtsRepairPlan,
) -> Result<FtsRepairApplyResult, FtsReconciliationError> {
    let tx = store.connection().unchecked_transaction()?;
    let mut result = FtsRepairApplyResult {
        rewritten_count: 0,
        skipped_count: 0,
        manual_review_count: 0,
    };

    for action in &plan.actions {
        match action.action {
            FtsRepairActionKind::RewriteFromCanonical => {
                if rewrite_chunk_fts_from_canonical(&tx, action)? {
                    result.rewritten_count += 1;
                } else {
                    result.skipped_count += 1;
                }
            }
            FtsRepairActionKind::ManualReviewOrphan => {
                result.manual_review_count += 1;
            }
        }
    }

    tx.commit()?;
    Ok(result)
}

fn action_sort_key(action: FtsRepairActionKind) -> u8 {
    match action {
        FtsRepairActionKind::RewriteFromCanonical => 0,
        FtsRepairActionKind::ManualReviewOrphan => 1,
    }
}

fn rewrite_chunk_fts_from_canonical(
    tx: &Transaction<'_>,
    action: &FtsRepairAction,
) -> Result<bool, FtsReconciliationError> {
    let canonical = load_canonical_row(tx, &action.document_key, &action.chunk_id)?;
    let observed = load_fts_rows(tx, &action.document_key, &action.chunk_id)?;

    if observed.len() == 1 && observed[0].matches_canonical(&canonical) {
        return Ok(false);
    }

    tx.execute(
        "
        DELETE FROM chunk_fts
        WHERE document_key = ?1 AND chunk_id = ?2
        ",
        params![action.document_key, action.chunk_id],
    )?;
    tx.execute(
        "
        INSERT INTO chunk_fts (
            document_key, chunk_id, source, title, body, page_start, page_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            canonical.document_key,
            canonical.chunk_id,
            canonical.source,
            canonical.title,
            canonical.body,
            canonical.page_start,
            canonical.page_end,
        ],
    )?;

    Ok(true)
}

fn load_canonical_row(
    tx: &Transaction<'_>,
    document_key: &str,
    chunk_id: &str,
) -> Result<CanonicalRepairRow, FtsReconciliationError> {
    let row = tx.query_row(
        "
        SELECT d.document_key, c.chunk_id, d.source, d.title, c.text, c.page_start, c.page_end
        FROM chunks c
        INNER JOIN documents d ON d.id = c.document_id
        WHERE d.document_key = ?1 AND c.chunk_id = ?2
        ",
        params![document_key, chunk_id],
        |row| {
            Ok(CanonicalRepairRow {
                document_key: row.get(0)?,
                chunk_id: row.get(1)?,
                source: row.get(2)?,
                title: row.get(3)?,
                body: row.get(4)?,
                page_start: row.get(5)?,
                page_end: row.get(6)?,
            })
        },
    )?;
    Ok(row)
}

fn load_fts_rows(
    tx: &Transaction<'_>,
    document_key: &str,
    chunk_id: &str,
) -> Result<Vec<FtsRepairRow>, FtsReconciliationError> {
    let mut statement = tx.prepare(
        "
        SELECT document_key, chunk_id, source, title, body, page_start, page_end
        FROM chunk_fts
        WHERE document_key = ?1 AND chunk_id = ?2
        ORDER BY rowid
        ",
    )?;
    let rows = statement.query_map(params![document_key, chunk_id], |row| {
        Ok(FtsRepairRow {
            document_key: row.get(0)?,
            chunk_id: row.get(1)?,
            source: row.get(2)?,
            title: row.get(3)?,
            body: row.get(4)?,
            page_start: row.get(5)?,
            page_end: row.get(6)?,
        })
    })?;

    let mut observed = Vec::new();
    for row in rows {
        observed.push(row?);
    }
    Ok(observed)
}

#[derive(Clone, Debug)]
struct CanonicalRepairRow {
    document_key: String,
    chunk_id: String,
    source: String,
    title: String,
    body: String,
    page_start: i64,
    page_end: i64,
}

#[derive(Clone, Debug)]
struct FtsRepairRow {
    document_key: String,
    chunk_id: String,
    source: String,
    title: String,
    body: String,
    page_start: i64,
    page_end: i64,
}

impl FtsRepairRow {
    fn matches_canonical(&self, canonical: &CanonicalRepairRow) -> bool {
        self.document_key == canonical.document_key
            && self.chunk_id == canonical.chunk_id
            && self.source == canonical.source
            && self.title == canonical.title
            && self.body == canonical.body
            && self.page_start == canonical.page_start
            && self.page_end == canonical.page_end
    }
}
