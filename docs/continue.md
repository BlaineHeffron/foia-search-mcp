# Continue

## Current State

Working checkpoint after `c34ca8b` (`feat(ingest): kick queued worker`).
This session adds focused executor-level resume coverage using the
existing durable job status, stage, progress, lease, and upsert fields.

Additional checkpoint after the OCR fallback seam slice:

- Added an opt-in PDF OCR fallback policy: `FOIA_SEARCH_OCR_FALLBACK=off|on_quality_warning`,
  defaulting to `off`.
- Added fake-testable OCR selection seams in `src/ingest/ocr.rs` and
  `src/ingest/pdf_text.rs`. PDF ingestion still extracts embedded text first.
  When embedded text has quality warnings and the policy is enabled, a caller-
  supplied OCR extractor may replace the page text only when page counts and
  page numbers match. If embedded extraction fails and policy is enabled, OCR may
  rescue the job. The default OCR extractor is a no-op unavailable extractor; no
  that checkpoint did not execute a real `ocrmypdf` or `tesseract` command; the
  later local `ocrmypdf` backend slice below wires the first real local OCR
  command behind the same default-off policy.
- Refactored ingestion persistence so the executor can persist already-selected
  extracted pages with the selected `TextSource`, preserving `local_ocr`
  provenance when the seam selects OCR.
- Added focused selector tests outside `src/ingest/executor_tests.rs`, including
  no-warning/no-OCR, disabled policy, enabled OCR, OCR failure fallback, embedded
  failure rescue, and non-PDF text asset bypass coverage.

Additional checkpoint after the redirect policy slice:

- Added explicit ingestion redirect policy types in `src/ingest/redirect.rs`.
  Source adapters now have a default-deny `redirect_policy()` hook, and CIA/NARA
  still inherit deny-by-default behavior.
- Asset downloads still build the reqwest client with `Policy::none()` and now
  manually follow only source-declared redirects after per-hop validation.
  Validation covers max hops, relative `Location` resolution, unsupported
  schemes, credentialed targets, cross-host redirects unless explicitly allowed,
  and obvious unsafe literal/metadata/local hosts on cross-host redirects.
- Added focused downloader tests for default deny with no blob/cache write,
  explicit same-host follow, max-hop enforcement, cross-host denial, relative
  redirect resolution, and unsafe redirect target rejection.
- Residual risk: this slice does not pin or verify the connected peer IP after
  DNS resolution, so DNS rebinding and other SSRF TOCTOU cases remain possible
  for future cross-host policies. Do not enable cross-host redirect following
  for a real source without a reviewed network-level mitigation.

Additional checkpoint after the local `ocrmypdf` backend slice:

- Added a real local OCR backend in `src/ingest/ocrmypdf.rs`. It runs
  `ocrmypdf` with structured command arguments in a private temp directory,
  enforces a timeout, captures bounded stderr, validates the OCR output PDF, and
  reparses the OCRed PDF through the existing `PdftotextExtractor` so page text
  normalization remains consistent.
- OCR remains default-off in two layers. `FOIA_SEARCH_OCR_FALLBACK` still
  defaults to `off`, and the backend defaults to `FOIA_SEARCH_OCR_BACKEND=none`.
  Production worker wiring uses the no-op OCR extractor unless both
  `FOIA_SEARCH_OCR_FALLBACK=on_quality_warning` and
  `FOIA_SEARCH_OCR_BACKEND=ocrmypdf` are set.
- Added backend configuration for `FOIA_SEARCH_OCRMYPDF_BIN`,
  `FOIA_SEARCH_OCR_TIMEOUT_SECONDS`, and
  `FOIA_SEARCH_OCR_MAX_STDERR_BYTES`. Tests use fake local executables and do
  not require `ocrmypdf` to be installed.

Additional checkpoint after the process-restart smoke-test slice:

- Added `tests/ingest_worker_restart.rs`, a deterministic process-boundary
  integration test that re-execs the test binary as child processes against a
  shared on-disk SQLite/blob fixture.
- The parent process seeds a queued job, blocks the first child mid-download on
  a controllable loopback text fixture, kills that child, expires the stale
  running lease, then launches a second child that reclaims and drains the same
  durable queue entry.
- Assertions cover durable running-stage/progress visibility before restart,
  resumed terminal success with incremented attempts, and idempotent persistence
  (no duplicate document/asset/page/chunk/FTS rows) without requiring live
  source HTTP or local `pdftotext`/`ocrmypdf` tooling.

Additional checkpoint after the process-restart partial-persistence slice:

- Extended restart coverage with a second deterministic process-boundary fixture
  in `tests/ingest_worker_restart.rs` plus a small
  `tests/ingest_worker_restart_support.rs` helper module to keep the main test
  file under the Rust size gate.
- The new fixture re-execs the test binary across child processes against a
  shared on-disk SQLite/blob fixture where a first child seeds stale local
  document/asset/page/chunk/FTS rows and marks the job as an expired running
  attempt at `extracting_text` progress `0.60`.
- A second child resumes through the real queued worker path and assertions now
  cover stage/progress/attempts transitions, replacement of stale persisted
  rows without duplicates, stale-term eviction from `chunk_fts`, and final page
  content matching fixture text without live source HTTP or local OCR/PDF
  binaries.

Additional checkpoint after the OCR mismatch warning slice:

- When embedded PDF extraction has quality warnings and OCR fallback is enabled,
  selector attempts that return OCR output with incompatible page counts/page
  numbers now add a warning instead of silently falling back.
- Citation safety behavior is unchanged: embedded text is still selected when
  OCR boundaries differ, preserving embedded page-number provenance.
- The incompatibility message now flows through existing extraction warnings to
  durable ingestion job warnings, with focused selector and executor-path tests
  in `src/ingest/pdf_text_tests.rs`.

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
- Executor resume tests now cover reclaiming an expired mid-stage running job,
  replacing stale partial document/page/chunk/FTS/asset state without duplicates,
  preserving interrupted stage/progress before resume, and failing after download
  without leaving local document/asset/page/chunk rows.

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

Then prioritize mid-job cancellation/interruption behavior so shutdown can mark
in-flight work resumable immediately instead of waiting for the active executor
iteration to finish.

## Next Tasks

- Add explicit redirect-follow policy if a future source requires redirects; validate each hop before enabling it.
- Add a later `tesseract` backend only if needed; the backend config now has a
  small enum shape that can support another local OCR backend.
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
