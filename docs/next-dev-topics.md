# Next Development Topics

Status as of `2faed39 fix(govinfo): use public PDF asset URLs`: the
worktree was clean, `main` was pushed to `origin/main`, and the latest source
expansion/live-smoke fixes were committed.

## Pick Up Here

1. Review the all-source live smoke results before adding new source behavior.
   Several public reading-room sites can return `403` to scripted clients; treat
   those as source/network/policy evidence unless the adapter is supposed to use
   a stable official API. The right follow-up is usually clearer structured
   status/guidance, not bypassing bot protection.
2. Continue lightweight live validation for every wired source:
   - API-backed or stable official endpoints should return usable records.
   - Manual/tracer/config-gated sources should fail gracefully with next steps.
   - Ingestion smoke tests should use isolated `FOIA_SEARCH_DATA_DIR` values.
3. Keep the GovInfo public-asset fix under observation. The latest slice changed
   GovInfo asset selection to prefer public PDF URLs instead of API asset URLs
   that can 403 during ingestion.
4. Decide whether to pause source expansion and start the Phase 9 indexing
   evaluation in `docs/phase9-indexing-evaluation.md`. SQLite FTS remains the
   canonical local-search backend until evals show a real weakness; LanceDB
   remains deferred until there is a defined embedding model and measurable
   gain.
5. If source expansion resumes, choose the next official source by research
   demand and source stability. Each source still needs fixtures, source
   warning/citation notes, cache and redirect policy contracts, and at least one
   eval.

## Current Constraints

- Run `git status --short --branch` before edits and never revert user work.
- Do not grow oversized or near-limit Rust files. In particular,
  `src/ingest/source_plan.rs` is at the size edge after the FRUS/GovInfo work;
  split future planning behavior into focused submodules or tests.
- Preserve source citation, terms, warning/caveat notes, official URLs, asset
  metadata, and cache policy through search, ingestion, and local retrieval.
- Keep redirects default-deny unless a specific future source requires an
  explicit reviewed policy with hop validation.
- Keep derived-artifact and SQLite FTS repair surfaces explicit
  report/plan/apply workflows only. Do not add startup, runtime, or worker
  auto-repair wiring.
- Keep `tesseract` unavailable unless a concrete OCR need justifies a real
  backend slice.

## Required Gates

Before commit or handoff, run the repo gates from `README.md` and `Justfile`:

```bash
just ai-gates
just fmt
just lint
just test
just architecture
git diff --check
```
