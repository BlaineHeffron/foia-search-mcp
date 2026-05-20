use super::search::{LocalSearchBackend, SearchHit, SearchQuery};
use crate::{model::source_warning_from_metadata, store::StoreError};
use serde_json::Value;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedSearchQuery {
    pub name: String,
    pub query: SearchQuery,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedSearchQuerySet {
    pub name: String,
    pub queries: Vec<NamedSearchQuery>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalSearchEvalReport {
    pub backend_name: String,
    pub query_set_name: String,
    pub query_reports: Vec<LocalSearchQueryReport>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalSearchQueryReport {
    pub query_name: String,
    pub query: SearchQuery,
    pub elapsed: Duration,
    pub hit_count: usize,
    pub is_empty: bool,
    pub empty_result_next_action: Option<String>,
    pub source_filter_matches_all_hits: bool,
    pub result_order: Vec<ObservedHitId>,
    pub hits: Vec<ObservedSearchHit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedHitId {
    pub document_key: String,
    pub chunk_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedSearchHit {
    pub rank: usize,
    pub id: ObservedHitId,
    pub source: String,
    pub title: String,
    pub score: f64,
    pub page_start: i64,
    pub page_end: i64,
    pub snippet: String,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
    pub source_warning: Option<String>,
    pub shape_parity: SearchHitShapeParity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHitShapeParity {
    pub has_document_key: bool,
    pub has_chunk_id: bool,
    pub has_page_range: bool,
    pub has_snippet: bool,
    pub has_citation_note: bool,
    pub has_terms_note: bool,
    pub has_source_warning: bool,
}

pub fn run_local_search_eval<B>(
    backend_name: impl Into<String>,
    query_set: &NamedSearchQuerySet,
    backend: &B,
) -> Result<LocalSearchEvalReport, StoreError>
where
    B: LocalSearchBackend + ?Sized,
{
    let query_reports = query_set
        .queries
        .iter()
        .map(|named_query| {
            let started_at = Instant::now();
            let hits = backend.search(&named_query.query)?;
            let elapsed = started_at.elapsed();
            Ok(build_query_report(named_query, hits, elapsed))
        })
        .collect::<Result<Vec<_>, StoreError>>()?;

    Ok(LocalSearchEvalReport {
        backend_name: backend_name.into(),
        query_set_name: query_set.name.clone(),
        query_reports,
    })
}

fn build_query_report(
    named_query: &NamedSearchQuery,
    hits: Vec<SearchHit>,
    elapsed: Duration,
) -> LocalSearchQueryReport {
    let source_filter = named_query.query.source.as_deref();
    let observed_hits = hits
        .into_iter()
        .enumerate()
        .map(|(index, hit)| observed_hit(index + 1, hit))
        .collect::<Vec<_>>();
    let source_filter_matches_all_hits = source_filter.is_none_or(|source| {
        observed_hits
            .iter()
            .all(|observed_hit| observed_hit.source == source)
    });
    let result_order = observed_hits
        .iter()
        .map(|hit| hit.id.clone())
        .collect::<Vec<_>>();
    let is_empty = observed_hits.is_empty();

    LocalSearchQueryReport {
        query_name: named_query.name.clone(),
        query: named_query.query.clone(),
        elapsed,
        hit_count: observed_hits.len(),
        is_empty,
        empty_result_next_action: is_empty.then(|| empty_result_next_action(source_filter)),
        source_filter_matches_all_hits,
        result_order,
        hits: observed_hits,
    }
}

fn empty_result_next_action(source_filter: Option<&str>) -> String {
    match source_filter {
        Some(source) => format!(
            "No local hits for source '{source}'. Ingest additional local documents for that source or broaden query terms."
        ),
        None => {
            "No local hits. Ingest local documents first or broaden query/source constraints."
                .to_owned()
        }
    }
}

fn observed_hit(rank: usize, hit: SearchHit) -> ObservedSearchHit {
    let source_warning = source_warning_from_metadata_json(&hit.metadata_json);
    let shape_parity = SearchHitShapeParity {
        has_document_key: !hit.document_key.as_str().is_empty(),
        has_chunk_id: !hit.chunk_id.trim().is_empty(),
        has_page_range: hit.page_start > 0 && hit.page_end >= hit.page_start,
        has_snippet: !hit.snippet.trim().is_empty(),
        has_citation_note: hit
            .citation_note
            .as_deref()
            .is_some_and(|note| !note.trim().is_empty()),
        has_terms_note: hit
            .terms_note
            .as_deref()
            .is_some_and(|note| !note.trim().is_empty()),
        has_source_warning: source_warning.is_some(),
    };

    ObservedSearchHit {
        rank,
        id: ObservedHitId {
            document_key: hit.document_key.to_string(),
            chunk_id: hit.chunk_id,
        },
        source: hit.source,
        title: hit.title,
        score: hit.score,
        page_start: hit.page_start,
        page_end: hit.page_end,
        snippet: hit.snippet,
        citation_note: hit.citation_note,
        terms_note: hit.terms_note,
        source_warning,
        shape_parity,
    }
}

fn source_warning_from_metadata_json(metadata_json: &str) -> Option<String> {
    serde_json::from_str::<Value>(metadata_json)
        .ok()
        .and_then(|metadata| source_warning_from_metadata(&metadata))
}
