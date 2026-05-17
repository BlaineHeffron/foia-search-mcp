use crate::ingest::reconcile::{
    DerivedArtifactIssueKind, DerivedArtifactKind, DerivedArtifactReport,
};
use crate::store::DocumentKey;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DerivedArtifactRewriteReason {
    Missing,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DerivedArtifactRepairAction {
    RewriteFromSqlite {
        kind: DerivedArtifactKind,
        path: PathBuf,
        page_number: Option<u32>,
        reason: DerivedArtifactRewriteReason,
    },
    ManualReview {
        kind: DerivedArtifactKind,
        path: PathBuf,
        page_number: Option<u32>,
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedArtifactRepairPlan {
    pub document_key: DocumentKey,
    pub actions: Vec<DerivedArtifactRepairAction>,
}

pub fn plan_derived_artifact_repairs(report: &DerivedArtifactReport) -> DerivedArtifactRepairPlan {
    let mut actions = report
        .issues
        .iter()
        .map(plan_issue)
        .collect::<Vec<DerivedArtifactRepairAction>>();
    actions.sort();

    DerivedArtifactRepairPlan {
        document_key: report.document_key.clone(),
        actions,
    }
}

fn plan_issue(
    issue: &crate::ingest::reconcile::DerivedArtifactIssue,
) -> DerivedArtifactRepairAction {
    match issue.issue {
        DerivedArtifactIssueKind::Missing => DerivedArtifactRepairAction::RewriteFromSqlite {
            kind: issue.kind,
            path: issue.path.clone(),
            page_number: issue.page_number,
            reason: DerivedArtifactRewriteReason::Missing,
        },
        DerivedArtifactIssueKind::Stale => DerivedArtifactRepairAction::RewriteFromSqlite {
            kind: issue.kind,
            path: issue.path.clone(),
            page_number: issue.page_number,
            reason: DerivedArtifactRewriteReason::Stale,
        },
        DerivedArtifactIssueKind::Orphaned => DerivedArtifactRepairAction::ManualReview {
            kind: issue.kind,
            path: issue.path.clone(),
            page_number: issue.page_number,
            detail: issue.detail.clone(),
        },
    }
}
