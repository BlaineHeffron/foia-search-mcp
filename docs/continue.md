# Continue

## Current State

Committed checkpoint: `e1c61cf` (`feat: add ingestion job and download foundations`).
`main` was pushed to `origin/main` after validation.

This batch combines the ingestion foundation slices:

- Durable ingestion job lifecycle APIs with leases, stages, progress, warnings, terminal states, and resume-oriented tests.
- Source-record planning from normalized `SourceRecord` values into ingestion documents and selected assets.
- Bounded asset downloading into the content-addressed file store with cache provenance, ETag/Last-Modified revalidation, and `DoNotPersist` cache semantics.
- External `pdftotext` extraction with structured command arguments, timeout handling, bounded stderr capture, temp output validation, and text-quality warnings.

Review fixes in this batch:

- Source planning now preserves full source metadata values in `metadata_json`, while still recording selected-asset planning details.
- The default downloader no longer follows redirects implicitly; redirects fail without writing a blob or cache row.

## Validation Commands

The checkpoint above passed these commands. Run them again before handing off or building the
next slice:

```bash
just ai-gates
just fmt
just lint
just test
just architecture
npm test
npm run build
git diff --check
```

Also keep running the final scans used for this batch: Rust LOC, production `unwrap|expect|panic|todo|unimplemented`, and outdated setup wording.

## First Action Next Session

Start with a read-only pass over these modules:

- `src/ingest/jobs.rs`
- `src/ingest/source_plan.rs`
- `src/ingest/download.rs`
- `src/ingest/pdftotext.rs`

Then design the queued ingestion executor that connects them. The first implementation step should
claim a queued job, resolve its source record, build an ingestion plan, download the selected asset,
extract text, and persist pages/chunks through the existing pipeline. Keep the first executor slice
single-worker and deterministic before adding background concurrency.

## Next Tasks

- Wire the source planner, downloader, job lifecycle, and text extractor into the queued ingestion executor.
- Persist downloaded asset rows after successful fetch/revalidation.
- Add explicit redirect-follow policy if a future source requires redirects; validate each hop before enabling it.
- Decide how local OCR fallback is selected when embedded PDF text produces quality warnings.
- Add crash/restart coverage for executor resume using existing job stage/progress fields.

## Constraints

- Do not grow existing oversized files such as `src/mcp/tools.rs`; add submodules for new behavior.
- Install hooks with `just install-hooks`; pre-commit runs `scripts/ai-dev-gates.sh --pre-commit`.
- Keep Rust modules under the gate limits. Existing oversized modules are frozen at their current
  line counts and must be split before adding behavior.
- Keep source IDs out of filesystem paths; use `document_key` and content hashes for local artifacts.
- Preserve source citation and terms notes with every ingested document.
- Treat TypeScript as draft/reference until removed; Rust is the primary implementation target.
- Do not enable redirects by default; add explicit hop validation before allowing source-specific redirects.
- Do not shell out through a string command for PDF/OCR work; use structured `Command` arguments only.
