# Continue

## Current State

This batch combines the ingestion foundation slices:

- Durable ingestion job lifecycle APIs with leases, stages, progress, warnings, terminal states, and resume-oriented tests.
- Source-record planning from normalized `SourceRecord` values into ingestion documents and selected assets.
- Bounded asset downloading into the content-addressed file store with cache provenance, ETag/Last-Modified revalidation, and `DoNotPersist` cache semantics.
- External `pdftotext` extraction with structured command arguments, timeout handling, bounded stderr capture, temp output validation, and text-quality warnings.

Review fixes in this batch:

- Source planning now preserves full source metadata values in `metadata_json`, while still recording selected-asset planning details.
- The default downloader no longer follows redirects implicitly; redirects fail without writing a blob or cache row.

## Validation Commands

Run these before handing off or building the next slice:

```bash
just fmt
just lint
just test
just architecture
npm test
npm run build
git diff --check
```

Also keep running the final scans used for this batch: Rust LOC, production `unwrap|expect|panic|todo|unimplemented`, and outdated setup wording.

## Next Tasks

- Wire the source planner, downloader, job lifecycle, and text extractor into the queued ingestion executor.
- Persist downloaded asset rows after successful fetch/revalidation.
- Add explicit redirect-follow policy if a future source requires redirects; validate each hop before enabling it.
- Decide how local OCR fallback is selected when embedded PDF text produces quality warnings.
- Add crash/restart coverage for executor resume using existing job stage/progress fields.

## Constraints

- Do not grow existing oversized files such as `src/mcp/tools.rs`; add submodules for new behavior.
- Keep source IDs out of filesystem paths; use `document_key` and content hashes for local artifacts.
- Preserve source citation and terms notes with every ingested document.
- Treat TypeScript as draft/reference until removed; Rust is the primary implementation target.
