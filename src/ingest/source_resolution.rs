use crate::ingest::{plan_source_ingestion, SourceIngestionPlan, SourcePlanError};
use crate::sources::{CachePolicy, SourceAdapter, SourceError, SourceRecord};
use std::fmt;

#[derive(Clone, Debug)]
pub struct ResolvedSourceRecord {
    record: SourceRecord,
    cache_policy: CachePolicy,
}

#[derive(Debug)]
pub enum SourceResolutionError {
    Source(SourceError),
}

impl fmt::Display for SourceResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SourceResolutionError {}

impl From<SourceError> for SourceResolutionError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

pub async fn resolve_source_record(
    adapter: &dyn SourceAdapter,
    id_or_url: &str,
) -> Result<ResolvedSourceRecord, SourceResolutionError> {
    let mut record = adapter.get_record(id_or_url).await?;
    record.attachments = adapter.list_assets(&record).await?;
    Ok(ResolvedSourceRecord {
        record,
        cache_policy: adapter.cache_policy(),
    })
}

pub fn plan_resolved_source_ingestion(
    resolved: ResolvedSourceRecord,
) -> Result<SourceIngestionPlan, SourcePlanError> {
    plan_source_ingestion(&resolved.record, resolved.cache_policy)
}
