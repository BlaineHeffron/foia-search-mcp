pub mod reconcile;
#[cfg(test)]
mod reconcile_tests;
pub mod sqlite_fts;

pub use reconcile::{
    reconcile_sqlite_fts_index, FtsReconciliationError, FtsReconciliationIssue,
    FtsReconciliationIssueKind, FtsReconciliationReport,
};
pub use sqlite_fts::{FtsSearch, SearchHit, SearchQuery};
