use crate::ingest::reconcile::{
    reconcile_derived_artifacts_for_document, DerivedArtifactIssueKind, DerivedArtifactKind,
    ReconcileError,
};
use crate::ingest::{
    apply_derived_artifact_repairs as apply_internal_derived_artifact_repairs,
    plan_derived_artifact_repairs as plan_internal_derived_artifact_repairs,
    DerivedArtifactApplyReport, DerivedArtifactRepairAction as InternalRepairAction,
    DerivedArtifactRepairPlan as InternalRepairPlan, DerivedArtifactRewriteReason,
    RepairApplyError,
};
use crate::model::{
    DerivedArtifactRepairAction, DerivedArtifactRepairApplyResponse, DerivedArtifactRepairIssue,
    DerivedArtifactRepairPlanResponse, DerivedArtifactRepairReportResponse,
};
use crate::store::{ContentAddressedStore, SqliteStore, StoreError};
use rmcp::ErrorData as McpError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepairSurfaceError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Reconcile(#[from] ReconcileError),

    #[error(transparent)]
    RepairApply(#[from] RepairApplyError),
}

impl RepairSurfaceError {
    pub fn into_mcp_error(self) -> McpError {
        match self {
            Self::InvalidRequest(message) => McpError::invalid_params(message, None),
            other => McpError::internal_error(other.to_string(), None),
        }
    }
}

pub fn report_derived_artifact_drift(
    store: &SqliteStore,
    files: &ContentAddressedStore,
    document_id: &str,
) -> Result<DerivedArtifactRepairReportResponse, RepairSurfaceError> {
    tracing::info!(document_id = %document_id, "report derived artifact drift");
    let report = reconcile_derived_artifacts_for_document(store, files, document_id)?;
    let next_actions = build_report_next_actions(&report.issues);
    Ok(DerivedArtifactRepairReportResponse {
        document_id: document_id.to_owned(),
        document_key: report.document_key.to_string(),
        issue_count: report.issues.len(),
        issues: report
            .issues
            .into_iter()
            .map(|issue| DerivedArtifactRepairIssue {
                kind: kind_label(issue.kind),
                issue: issue_label(issue.issue),
                path: issue.path.display().to_string(),
                page_number: issue.page_number,
                detail: issue.detail,
            })
            .collect(),
        next_actions,
    })
}

pub fn plan_derived_artifact_repairs(
    store: &SqliteStore,
    files: &ContentAddressedStore,
    document_id: &str,
) -> Result<DerivedArtifactRepairPlanResponse, RepairSurfaceError> {
    tracing::info!(document_id = %document_id, "plan derived artifact repairs");
    let report = reconcile_derived_artifacts_for_document(store, files, document_id)?;
    let plan = plan_internal_derived_artifact_repairs(&report);
    Ok(plan_to_response(document_id, plan))
}

pub fn apply_derived_artifact_repairs(
    store: &SqliteStore,
    files: &ContentAddressedStore,
    document_id: &str,
    confirmation: &str,
) -> Result<DerivedArtifactRepairApplyResponse, RepairSurfaceError> {
    let expected_confirmation = expected_confirmation(document_id);
    if confirmation != expected_confirmation {
        return Err(RepairSurfaceError::InvalidRequest(format!(
            "confirmation must exactly match '{expected_confirmation}'"
        )));
    }

    tracing::info!(document_id = %document_id, "apply derived artifact repairs");
    let report = reconcile_derived_artifacts_for_document(store, files, document_id)?;
    let plan = plan_internal_derived_artifact_repairs(&report);
    let apply_report = apply_internal_derived_artifact_repairs(store, files, &plan)?;
    Ok(DerivedArtifactRepairApplyResponse {
        document_id: document_id.to_owned(),
        document_key: plan.document_key.to_string(),
        issue_count: report.issues.len(),
        rewritten: apply_report.rewritten,
        already_current: apply_report.already_current,
        skipped_manual_review: apply_report.skipped_manual_review,
        next_actions: build_apply_next_actions(&report.issues, &apply_report),
    })
}

fn plan_to_response(
    document_id: &str,
    plan: InternalRepairPlan,
) -> DerivedArtifactRepairPlanResponse {
    let rewrite_count = plan
        .actions
        .iter()
        .filter(|action| matches!(action, InternalRepairAction::RewriteFromSqlite { .. }))
        .count();
    let manual_review_count = plan.actions.len().saturating_sub(rewrite_count);
    let next_actions = build_plan_next_actions(document_id, &plan.actions);

    DerivedArtifactRepairPlanResponse {
        document_id: document_id.to_owned(),
        document_key: plan.document_key.to_string(),
        action_count: plan.actions.len(),
        rewrite_count,
        manual_review_count,
        actions: plan
            .actions
            .into_iter()
            .map(|action| match action {
                InternalRepairAction::RewriteFromSqlite {
                    kind,
                    path,
                    page_number,
                    reason,
                } => DerivedArtifactRepairAction {
                    kind: kind_label(kind),
                    action: "rewrite_from_sqlite".to_owned(),
                    path: path.display().to_string(),
                    page_number,
                    reason: Some(reason_label(reason)),
                    detail: None,
                },
                InternalRepairAction::ManualReview {
                    kind,
                    path,
                    page_number,
                    detail,
                } => DerivedArtifactRepairAction {
                    kind: kind_label(kind),
                    action: "manual_review".to_owned(),
                    path: path.display().to_string(),
                    page_number,
                    reason: None,
                    detail: Some(detail),
                },
            })
            .collect(),
        next_actions,
    }
}

fn build_report_next_actions(
    issues: &[crate::ingest::reconcile::DerivedArtifactIssue],
) -> Vec<String> {
    let mut next_actions = Vec::new();
    if issues.is_empty() {
        next_actions.push("No derived artifact drift detected.".to_owned());
        return next_actions;
    }

    if issues
        .iter()
        .any(|issue| issue.issue == DerivedArtifactIssueKind::Orphaned)
    {
        next_actions.push(
            "Plan the drift before applying; orphaned artifacts are manual-review only.".to_owned(),
        );
    } else {
        next_actions.push("Plan the drift before applying.".to_owned());
    }

    next_actions
}

fn build_plan_next_actions(document_id: &str, actions: &[InternalRepairAction]) -> Vec<String> {
    let mut next_actions = Vec::new();
    if actions.is_empty() {
        next_actions.push("No repair actions were required.".to_owned());
        return next_actions;
    }

    let confirmation = expected_confirmation(document_id);
    if actions
        .iter()
        .any(|action| matches!(action, InternalRepairAction::ManualReview { .. }))
    {
        next_actions.push(format!(
            "Review manual-review items first; apply skips them. To rewrite safe items, confirm: {confirmation}"
        ));
    } else {
        next_actions.push(format!(
            "Apply only if the scope is correct, with confirmation: {confirmation}"
        ));
    }

    next_actions
}

fn build_apply_next_actions(
    issues: &[crate::ingest::reconcile::DerivedArtifactIssue],
    apply_report: &DerivedArtifactApplyReport,
) -> Vec<String> {
    let mut next_actions = Vec::new();
    if apply_report.skipped_manual_review > 0 {
        next_actions.push(
            "Manual-review items remain; inspect the report and resolve those artifacts separately."
                .to_owned(),
        );
    }
    if issues.is_empty() {
        next_actions.push("No derived artifact drift was present.".to_owned());
    } else if apply_report.already_current > 0 && apply_report.rewritten == 0 {
        next_actions.push("All applicable repair targets were already current.".to_owned());
    } else {
        next_actions
            .push("Re-run report or plan if the SQLite source data has changed.".to_owned());
    }
    next_actions
}

fn expected_confirmation(document_id: &str) -> String {
    format!("apply derived artifact repairs for {document_id}")
}

fn kind_label(kind: DerivedArtifactKind) -> String {
    match kind {
        DerivedArtifactKind::DocumentText => "document_text".to_owned(),
        DerivedArtifactKind::PageText => "page_text".to_owned(),
        DerivedArtifactKind::OcrPageText => "ocr_page_text".to_owned(),
    }
}

fn issue_label(issue: DerivedArtifactIssueKind) -> String {
    match issue {
        DerivedArtifactIssueKind::Missing => "missing".to_owned(),
        DerivedArtifactIssueKind::Stale => "stale".to_owned(),
        DerivedArtifactIssueKind::Orphaned => "orphaned".to_owned(),
    }
}

fn reason_label(reason: DerivedArtifactRewriteReason) -> String {
    match reason {
        DerivedArtifactRewriteReason::Missing => "missing".to_owned(),
        DerivedArtifactRewriteReason::Stale => "stale".to_owned(),
    }
}
