use crate::index::{
    apply_sqlite_fts_repair_plan as apply_internal_sqlite_fts_repair_plan,
    plan_sqlite_fts_repairs as plan_internal_sqlite_fts_repairs, reconcile_sqlite_fts_index,
    FtsReconciliationError, FtsReconciliationIssue, FtsReconciliationIssueKind, FtsRepairAction,
    FtsRepairActionKind, FtsRepairApplyResult, FtsRepairPlan,
};
use crate::model::{
    SqliteFtsRepairAction, SqliteFtsRepairApplyResponse, SqliteFtsRepairIssue,
    SqliteFtsRepairPlanResponse, SqliteFtsRepairReportResponse,
};
use crate::store::SqliteStore;
use rmcp::ErrorData as McpError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FtsRepairSurfaceError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error(transparent)]
    Reconcile(#[from] FtsReconciliationError),
}

impl FtsRepairSurfaceError {
    pub fn into_mcp_error(self) -> McpError {
        match self {
            Self::InvalidRequest(message) => McpError::invalid_params(message, None),
            other => McpError::internal_error(other.to_string(), None),
        }
    }
}

pub fn report_sqlite_fts_drift(
    store: &SqliteStore,
) -> Result<SqliteFtsRepairReportResponse, FtsRepairSurfaceError> {
    tracing::info!("report sqlite fts drift");
    let report = reconcile_sqlite_fts_index(store)?;
    let next_actions = build_report_next_actions(&report.issues);

    Ok(SqliteFtsRepairReportResponse {
        canonical_chunk_count: report.canonical_chunk_count,
        chunk_fts_row_count: report.chunk_fts_row_count,
        issue_count: report.issues.len(),
        issues: report
            .issues
            .into_iter()
            .map(|issue| SqliteFtsRepairIssue {
                issue: issue_label(issue.issue),
                document_key: issue.document_key,
                chunk_id: issue.chunk_id,
                detail: issue.detail,
            })
            .collect(),
        next_actions,
    })
}

pub fn plan_sqlite_fts_repairs(
    store: &SqliteStore,
) -> Result<SqliteFtsRepairPlanResponse, FtsRepairSurfaceError> {
    tracing::info!("plan sqlite fts repairs");
    let report = reconcile_sqlite_fts_index(store)?;
    let plan = plan_internal_sqlite_fts_repairs(&report);
    Ok(plan_to_response(plan))
}

pub fn apply_sqlite_fts_repairs(
    store: &SqliteStore,
    confirmation: &str,
) -> Result<SqliteFtsRepairApplyResponse, FtsRepairSurfaceError> {
    let expected_confirmation = expected_confirmation();
    if confirmation != expected_confirmation {
        return Err(FtsRepairSurfaceError::InvalidRequest(format!(
            "confirmation must exactly match '{expected_confirmation}'"
        )));
    }

    tracing::info!("apply sqlite fts repairs");
    let report = reconcile_sqlite_fts_index(store)?;
    let plan = plan_internal_sqlite_fts_repairs(&report);
    let apply_report = apply_internal_sqlite_fts_repair_plan(store, &plan)?;
    Ok(SqliteFtsRepairApplyResponse {
        issue_count: report.issues.len(),
        rewritten: apply_report.rewritten_count,
        already_current: apply_report.skipped_count,
        skipped_manual_review: apply_report.manual_review_count,
        next_actions: build_apply_next_actions(&report.issues, &apply_report),
    })
}

fn plan_to_response(plan: FtsRepairPlan) -> SqliteFtsRepairPlanResponse {
    let rewrite_count = plan
        .actions
        .iter()
        .filter(|action| action.action == FtsRepairActionKind::RewriteFromCanonical)
        .count();
    let manual_review_count = plan.actions.len().saturating_sub(rewrite_count);
    let next_actions = build_plan_next_actions(&plan.actions);

    SqliteFtsRepairPlanResponse {
        action_count: plan.actions.len(),
        rewrite_count,
        manual_review_count,
        actions: plan
            .actions
            .into_iter()
            .map(|action| SqliteFtsRepairAction {
                action: action_label(action.action),
                document_key: action.document_key,
                chunk_id: action.chunk_id,
                reason: action.reason,
            })
            .collect(),
        next_actions,
    }
}

fn build_report_next_actions(issues: &[FtsReconciliationIssue]) -> Vec<String> {
    if issues.is_empty() {
        return vec!["No SQLite FTS index drift detected.".to_owned()];
    }

    if issues
        .iter()
        .any(|issue| issue.issue == FtsReconciliationIssueKind::Orphaned)
    {
        vec![
            "Plan the drift before applying; orphaned chunk_fts rows are manual-review only."
                .to_owned(),
        ]
    } else {
        vec!["Plan the drift before applying.".to_owned()]
    }
}

fn build_plan_next_actions(actions: &[FtsRepairAction]) -> Vec<String> {
    if actions.is_empty() {
        return vec!["No repair actions were required.".to_owned()];
    }

    let confirmation = expected_confirmation();
    if actions
        .iter()
        .any(|action| action.action == FtsRepairActionKind::ManualReviewOrphan)
    {
        vec![format!(
            "Review manual-review items first; apply skips them. To rewrite safe items, confirm: {confirmation}"
        )]
    } else {
        vec![format!(
            "Apply only if the scope is correct, with confirmation: {confirmation}"
        )]
    }
}

fn build_apply_next_actions(
    issues: &[FtsReconciliationIssue],
    apply_report: &FtsRepairApplyResult,
) -> Vec<String> {
    let mut next_actions = Vec::new();
    if apply_report.manual_review_count > 0 {
        next_actions.push(
            "Manual-review chunk_fts rows remain; inspect the report and resolve them separately."
                .to_owned(),
        );
    }
    if issues.is_empty() {
        next_actions.push("No SQLite FTS index drift was present.".to_owned());
    } else if apply_report.skipped_count > 0 && apply_report.rewritten_count == 0 {
        next_actions.push("All applicable repair targets were already current.".to_owned());
    } else {
        next_actions
            .push("Re-run report or plan if the canonical SQLite chunks have changed.".to_owned());
    }
    next_actions
}

fn expected_confirmation() -> &'static str {
    "apply sqlite fts repairs"
}

fn issue_label(issue: FtsReconciliationIssueKind) -> String {
    match issue {
        FtsReconciliationIssueKind::Missing => "missing".to_owned(),
        FtsReconciliationIssueKind::Orphaned => "orphaned".to_owned(),
        FtsReconciliationIssueKind::Stale => "stale".to_owned(),
    }
}

fn action_label(action: FtsRepairActionKind) -> String {
    match action {
        FtsRepairActionKind::RewriteFromCanonical => "rewrite_from_canonical".to_owned(),
        FtsRepairActionKind::ManualReviewOrphan => "manual_review".to_owned(),
    }
}
