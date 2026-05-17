# Continue

## Current State

Working checkpoint after `240729e` (`feat: wire queued ingestion worker`).
This session adds an explicit MCP enqueue-to-worker kick path so queued ingestion
does not wait for the bounded idle polling interval when the worker is alive.

Current ingestion slices now include:

- Durable ingestion job lifecycle APIs with leases, stages, progress, warnings, terminal states, and resume-oriented tests.
- Source-record planning from normalized `SourceRecord` values into ingestion documents and selected assets.
- Bounded asset downloading into the content-addressed file store with cache provenance, ETag/Last-Modified revalidation, and `DoNotPersist` cache semantics.
- External `pdftotext` extraction with structured command arguments, timeout handling, bounded stderr capture, temp output validation, and text-quality warnings.
- `QueuedIngestionExecutor` claims durable jobs, resolves source records, plans ingestion, downloads selected assets, extracts text, persists pages/chunks, links assets, and records terminal job state.
- A single background ingestion worker is started by the Rust runtime. It opens a fresh SQLite handle per iteration, advances one queued job at a time, polls at a bounded interval when idle, and stops cleanly between iterations when the MCP service exits.

Review fixes already included:

- Source planning preserves full source metadata values in `metadata_json`, while still recording selected-asset planning details.
- The default downloader no longer follows redirects implicitly; redirects fail without writing a blob or cache row.
- MCP `ingest_document` and `refresh_document` still enqueue durable jobs and return quickly; job status output no longer says the worker is unwired.
- MCP `ingest_document` and `refresh_document` now notify the background worker after
  the durable job row is created. If that in-memory kick is missed or the worker is
  stopped, the durable queue remains authoritative and bounded polling still picks
  up the job.

## Validation Commands

Run these before handing off or building the next slice:

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

- `src/ingest/worker.rs`
- `src/ingest/executor.rs`
- `src/runtime.rs`
- `src/mcp/tools.rs`

Then validate end-to-end ingestion against a real CIA PDF in a throwaway `FOIA_SEARCH_DATA_DIR`, including immediate worker wakeup after MCP enqueue and behavior when `pdftotext` is missing or returns low-quality text.

## Next Tasks

- Add explicit redirect-follow policy if a future source requires redirects; validate each hop before enabling it.
- Decide how local OCR fallback is selected when embedded PDF text produces quality warnings.
- Add crash/restart coverage for executor resume using existing job stage/progress fields.
- Consider moving executor download cache writes out of the async HTTP boundary so the executor future can be `Send`; the current runtime uses a dedicated current-thread worker to avoid moving a SQLite handle across threads.
- Add mid-job cancellation/interruption. Current shutdown waits for the active executor iteration to finish; it does not mark an in-flight job interrupted immediately when the MCP process is asked to stop.

## Constraints

- Do not grow existing oversized files such as `src/mcp/tools.rs`; add submodules for new behavior.
- Install hooks with `just install-hooks`; pre-commit runs `scripts/ai-dev-gates.sh --pre-commit`.
- Keep Rust modules under the gate limits. Existing oversized modules are frozen at their current line counts and must be split before adding behavior.
- Keep source IDs out of filesystem paths; use `document_key` and content hashes for local artifacts.
- Preserve source citation and terms notes with every ingested document.
- Treat TypeScript as draft/reference until removed; Rust is the primary implementation target.
- Do not enable redirects by default; add explicit hop validation before allowing source-specific redirects.
- Do not shell out through a string command for PDF/OCR work; use structured `Command` arguments only.
