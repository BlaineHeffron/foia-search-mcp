# Phase 9 Indexing Evaluation Plan

## Reader And Decision

This note is for the engineer deciding whether to start Phase 9 work on
advanced local indexing.

The post-read action is simple: decide whether SQLite FTS is still sufficient
for the current corpus, or whether Tantivy should be evaluated as the next
index backend.

## Decision Rule

Start the evaluation only if SQLite FTS is showing a real limitation, not just
because Tantivy exists.

SQLite FTS is insufficient when one or more of these become true:

- Relevance order is no longer stable on a fixed local-search eval set.
- Query latency or index rebuild time is becoming a practical bottleneck for the
  current corpus and ingest rate.
- The search shape needs ranking behavior that FTS5 cannot express cleanly,
  such as better field-aware ranking or more controlled score shaping.
- The local search contract needs to preserve the same metadata, snippets, page
  ranges, citation notes, terms notes, and source warnings, but FTS output is no
  longer good enough for the target corpus.

If none of those are true, keep SQLite FTS as the local search backend and do
not add a second index just because it is available.

## Implementation Seam

The smallest future implementation seam is the read path.

Keep SQLite `documents`, `pages`, and `chunks` canonical. Treat any search
backend as a derived reader over that canonical data, not as the source of
truth.

The first code step should be a narrow local-search abstraction that exposes the
current `SearchQuery -> Vec<SearchHit>` contract. A SQLite implementation can
wrap the existing FTS path, and a Tantivy implementation can read from canonical
SQLite rows or a snapshot without changing ingestion durability.

Keep citation and warning shaping outside the backend. The backend should keep
returning the same metadata, citation note, terms note, and source-warning
inputs; MCP output shaping stays centralized.

## What Tantivy Must Prove

The comparison should measure the things that matter to the product, not raw
index novelty.

Measure:

- Relevance quality on a fixed query set with known good results.
- Top-k ordering for mixed-source corpora, especially cases where the same terms
  appear across several sources.
- Query latency at realistic corpus size, with both cold and warm runs.
- Index build and rebuild time after an ingestion batch.
- Storage overhead relative to the current SQLite FTS footprint.
- Output parity for document key, chunk ID, page range, snippet, citation note,
  terms note, and source warning.
- Behavior on empty corpora, partial rebuilds, and repeated rebuilds.

The comparison should answer one question: does Tantivy improve the user-facing
search result enough to justify the extra moving parts?

## Why LanceDB Is Deferred

LanceDB should stay out of Phase 9 until there is a defined embedding model and
a measurable semantic-search gain.

That deferral is intentional:

- The current problem is lexical retrieval over source-cited documents, not
  embedding governance.
- Semantic search changes the evaluation problem, the safety story, and the
  acceptance criteria at the same time.
- There is no reason to add a vector backend until the project can show that it
  improves a search task the current lexical index cannot already handle.

If a future milestone wants semantic search, it should bring a model choice,
collection shape, and eval gain statement with it.

## Eval Additions Needed

The eval set should grow in a way that makes the indexing decision observable.

Add eval coverage for:

- Local search ranking on a mixed-source corpus with overlapping keywords.
- Search result shaping, including snippets, page ranges, citation notes,
  terms notes, and source warnings.
- Source-filtered local search so the backend cannot win by ignoring filters.
- Empty or sparse corpus handling, with a clear next action instead of a silent
  failure.
- A comparison harness that can run the same query against SQLite FTS and the
  candidate Tantivy path and record the result order and latency.

If Tantivy is added, keep parity tests around the current contract so result
shape does not regress while the backend changes.

## Repair Surface Rules

Index repair must stay operator-confirmed.

The rules are the same as the current derived-artifact and SQLite FTS repair
surfaces:

- Provide report, plan, and apply steps.
- Require an exact confirmation string before apply.
- Keep report and plan read-only.
- Keep orphaned rows or other ambiguous cases as manual review only.
- Do not wire repair into startup, runtime recovery, or worker auto-repair.

If Phase 9 introduces index artifacts for Tantivy, those artifacts should follow
the same explicit operator-confirmed boundary rather than inventing an automatic
repair path.

Do not let ingestion workers silently rebuild a future Tantivy index. If the
project ever persists such an index on disk, it should get the same explicit
report/plan/apply treatment as derived text/OCR and SQLite FTS drift.

## Exit Criteria

Phase 9 evaluation is complete when:

- SQLite FTS has a measured weakness, or the baseline shows it is still
  sufficient and no backend change is justified.
- The comparison data says whether Tantivy is a net improvement for this corpus.
- The eval set covers ranking, shape, and repair boundaries well enough to keep
  the decision stable.
- LanceDB still has a documented reason to wait.

The eval contract should also pin the architecture boundary:

- SQLite remains canonical.
- Tantivy is derived and evaluation-gated.
- No startup, runtime, or worker auto-repair is allowed for future advanced
  index artifacts.

If the answer is still "SQLite FTS is good enough," stop there and keep the
system simple.
