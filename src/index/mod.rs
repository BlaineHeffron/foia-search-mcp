pub mod eval_report;
#[cfg(test)]
mod eval_report_tests;
pub mod reconcile;
pub mod reconcile_repair;
#[cfg(test)]
mod reconcile_repair_tests;
#[cfg(test)]
mod reconcile_tests;
pub mod search;
pub mod sqlite_fts;
#[cfg(test)]
mod sqlite_fts_eval_tests;
#[cfg(test)]
mod sqlite_sufficiency_eval_tests;

pub use eval_report::{
    run_local_search_eval, LocalSearchEvalReport, LocalSearchQueryReport, NamedSearchQuery,
    NamedSearchQuerySet, ObservedHitId, ObservedSearchHit, SearchHitShapeParity,
};
pub use reconcile::{
    reconcile_sqlite_fts_index, FtsReconciliationError, FtsReconciliationIssue,
    FtsReconciliationIssueKind, FtsReconciliationReport,
};
pub use reconcile_repair::{
    apply_sqlite_fts_repair_plan, plan_sqlite_fts_repairs, FtsRepairAction, FtsRepairActionKind,
    FtsRepairApplyResult, FtsRepairPlan,
};
pub use search::{LocalSearchBackend, LocalSearchIndex, SearchHit, SearchQuery};
pub use sqlite_fts::FtsSearch;
