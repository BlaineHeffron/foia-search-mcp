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
- Redirect following remains disabled by default across the current source set;
  no source opts in yet, so the live behavior is still default-deny.
- Residual risk: this slice does not pin or verify the connected peer IP after
  DNS resolution, so DNS rebinding and other SSRF TOCTOU cases remain possible
  for future cross-host policies. Do not enable cross-host redirect following
  for a real source without a reviewed network-level mitigation.

Additional checkpoint after the source cache-policy contract slice:

- Pinned the `SourceAdapter::cache_policy()` default as
  `RespectSourceHeaders` with focused source contract tests.
- CIA remains explicitly `RespectSourceHeaders`, while NARA still overrides to
  `DoNotPersist`; adding the trait default does not loosen NARA persistence.

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

Additional checkpoint after the mid-job cancellation slice:

- Added explicit cancellation tokens/checkpoints for queued ingestion execution.
  Worker shutdown requests cancellation before joining the worker thread, and
  claimed active jobs transition to `interrupted` with resume-oriented
  `next_action` text instead of staying `running` or becoming terminal failures.
- Executor cancellation checkpoints now cover claim, source resolution,
  planning, download boundaries, extraction start, extraction cancellation,
  document persistence, and final asset provenance writes. Cancellations before
  persistence leave no local rows; cancellations after document persistence keep
  document/page/chunk rows resumable and omit final asset provenance until a
  later reclaim completes the job.
- External `pdftotext` and `ocrmypdf` execution now share a cancellable child
  wait helper. Cancellation kills the child process group on Unix, returns a
  distinct `TextExtraction::Cancelled`, and preserves existing timeout behavior.
  Tests use fake local binaries and loopback HTTP fixtures only.
- The worker OCR adapter moved to `src/ingest/worker_ocr.rs` so cancellation
  propagation does not grow the near-limit `src/ingest/worker.rs` hotspot.

Additional checkpoint after the download-cache boundary slice:

- Split downloader cache policy/provenance/persistence handling into
  `src/ingest/download_persist.rs` so cache behavior stays isolated from the
  HTTP/body acquisition path.
- `AssetDownloader` now supports a two-phase flow (`load_cached_entry` ->
  async `download_http` -> sync `persist_prepared_download`) and the executor
  now uses that flow. This removes the SQLite-backed `CacheStore` borrow from
  the async HTTP await boundary while preserving existing semantics for
  redirects, ETag/Last-Modified revalidation, `DoNotPersist`, and
  header-driven `RespectSourceHeaders` behavior.
- Downloader integration coverage remains in `tests/download_cache.rs`, and
  focused helper tests now cover header-driven do-not-persist policy and cache
  row deletion for do-not-persist persistence.

Additional checkpoint after the source-resolution await boundary slice:

- Added a dedicated `src/ingest/source_resolution.rs` helper that resolves
  source records (`get_record`), resolves adapter-declared assets
  (`list_assets`), and hands the resolved record to the synchronous
  `SourceIngestionPlan` builder before document/download persistence work.
- `QueuedIngestionExecutor` now calls the helper between explicit durable stage
  updates (`resolving_source_record` -> `planning_ingestion`), keeping the
  source HTTP/listing awaits isolated from blocking store mutations and making
  the remaining non-`Send` boundary easier to reason about.
- Added focused cancellation/resume coverage in
  `src/ingest/executor_cancel_tests.rs` proving interruption at
  `AfterSourceResolution` keeps durable stage/progress, leaves no partial local
  rows, and resumes idempotently on reclaim.

Additional checkpoint after the executor store-boundary trial slice:

- Added a focused `src/ingest/executor_async.rs` helper boundary for awaited
  source resolution and awaited HTTP download that uses owned request structs
  and keeps `&mut SqliteStore` out of those awaited futures.
- `QueuedIngestionExecutor` now builds and persists cache/document/job mutations
  in synchronous sections around those awaited helper boundaries, preserving
  durable stage progression, cancellation checkpoints, warnings, interruption,
  and failure semantics.
- Added `src/ingest/executor_send_tests.rs` with compile-time `Send` assertions
  for the new store-free awaited boundary futures, and kept
  `src/ingest/executor_tests.rs` under the size gate by moving this coverage
  into the new focused module.
- Current blocker: top-level `run_next*`/`execute_claimed_job` async methods
  still take `&mut SqliteStore`, so their futures are still not `Send` yet even
  though the awaited network boundaries are now store-free.

Additional checkpoint after the executor full-send signature slice:

- Refactored async executor method signatures to remove `&mut SqliteStore`
  parameters. `run_next*` and `execute_claimed_job` now take owned
  `SqliteStore` values and return the store alongside execution results so
  caller-visible state/inspection semantics stay intact across both success and
  failure paths.
- Kept durable stage/progress/warning/interruption/failure/idempotent behavior
  intact while preserving store mutation ordering around source resolution,
  download, extraction, and provenance writes.
- Tightened executor boundary trait-object requirements to
  `&(dyn TextExtractor + Sync)` and `&(dyn CancellationSignal + Sync)`,
  including test fixtures, so executor futures can satisfy compile-time Send
  checks.
- Extended `src/ingest/executor_send_tests.rs` with a compile-time assertion for
  the full `run_next` executor future; this assertion now passes.

Additional checkpoint after the runtime follow-up evaluation slice:

- Evaluated the next runtime hardening step after full executor `Send` coverage
  and kept the queued worker execution model unchanged: a dedicated worker
  thread with a current-thread Tokio runtime still provides the smallest,
  safest path for existing kick/poll/shutdown behavior.
- Added `src/ingest/worker_send_tests.rs` with a compile-time `Send` assertion
  for `QueuedIngestionWorker::run_once()` so worker future boundaries now
  explicitly prove the executor-level `Send` property is preserved at the
  runtime seam.
- Remaining runtime risk: this slice intentionally does not migrate queued work
  onto a shared multithread Tokio runtime because that would be an architecture
  change requiring broader cancellation/kick/shutdown regression coverage.

Additional checkpoint after the GovInfo tracer registration slice:

- Added `src/sources/govinfo.rs` as a narrow, disabled/manual tracer adapter for
  the next planned source after CIA/NARA. The tracer intentionally does not make
  live GovInfo API calls yet and returns actionable guidance that points callers
  to official GovInfo Search Service and package/granule summary concepts.
- Registered the GovInfo tracer in runtime source wiring without touching CIA or
  NARA hotspot modules; `search_source`/`get_source_record` for `govinfo` now
  fail fast with explicit manual-next-step guidance instead of source-unavailable.
- Pinned GovInfo source policy contracts via focused tests so default
  `cache_policy=RespectSourceHeaders` and `redirect_policy=Deny` remain locked
  for this source while the live API slice is designed.
- Updated `Config::source_status()` GovInfo note/status to reflect
  `manual_tracer` behavior and official API link preference (PDF/XML/MODS).

Additional checkpoint after the GovInfo live API adapter slice:

- Replaced the disabled/manual GovInfo tracer with a live API adapter using the
  official Search Service and package/granule summary endpoints.
- Added deterministic loopback HTTP tests and GovInfo fixtures for search,
  empty results, package summary, granule summary, invalid HTML/source-changed
  responses, and redirect denial.
- GovInfo remains on the default source policy contracts:
  `cache_policy=RespectSourceHeaders` and `redirect_policy=Deny`.
- Updated user-facing source status/tool wording so `list_sources`,
  `search_source`, and `get_source_record` no longer describe GovInfo as manual.

Additional checkpoint after the DOE OpenNet adapter slice:

- Verified official DOE/OSTI OpenNet entry points at `https://www.osti.gov/opennet/`,
  `https://www.osti.gov/opennet/faq`, and `https://www.osti.gov/opennet/order`.
  The site describes OpenNet as a Department of Energy supported declassified-
  records database with official search and detail pages, while noting not all
  records have electronic full text.
- Added a conservative fixture-backed `doe` adapter for OpenNet search/detail
  leads. It uses official OpenNet POST search and detail pages, returns stable
  `doe:<osti-id>` records, preserves accession/location/source-warning
  metadata, keeps default `RespectSourceHeaders` cache policy and `Deny`
  redirect policy, and treats page citations as requiring PDF ingestion and
  page-boundary verification.
- Added deterministic loopback tests and fixtures for search, get_record,
  empty results, source-changed markup, asset listing with PDF-first ordering,
  official URL validation, and redirect denial.

Additional checkpoint after the NSA FOIA Reading Room adapter slice:

- Added `src/sources/nsa.rs` and split live helper modules for an official
  `nsa` adapter targeting `https://www.nsa.gov/Helpful-Links/NSA-FOIA/Reading-Room/`
  plus the official FOIA Reports and Releases list.
- The adapter remains conservative: source search parses official NSA page
  links from deterministic fixtures, `get_record` accepts returned source IDs
  or official NSA URLs, and assets are ordered PDF-first while non-PDF links are
  retained as metadata leads.
- NSA keeps default source policy contracts: `cache_policy=RespectSourceHeaders`
  and `redirect_policy=Deny`.

Additional checkpoint after the State Department Virtual Reading Room adapter slice:

- Added `src/sources/state.rs` and split live helper modules for an official
  `state` adapter targeting `https://foia.state.gov/` and the Virtual Reading
  Room/Search Released Documents entry point at `/Search/Results.aspx`.
- The adapter is conservative and fixture-backed: source search parses official
  `foia.state.gov` search-result/detail/PDF links, `get_record` accepts returned
  source IDs or official URLs, and `list_assets` prefers PDFs while retaining
  HTML/OCR-text/non-PDF assets as metadata leads.
- State keeps default source policy contracts:
  `cache_policy=RespectSourceHeaders` and `redirect_policy=Deny`. Source
  warnings preserve State's OCR caveat, unavailable-field caveat, and
  originating-agency caveat; page citations still require PDF ingestion and
  page-boundary verification.

Additional checkpoint after the DIA FOIA Electronic Reading Room adapter slice:

- Added `src/sources/dia.rs` and split live helper modules for the official
  DIA FOIA Electronic Reading Room entry point at
  `https://www.dia.mil/FOIA/FOIA-Electronic-Reading-Room/`.
- The adapter is conservative and fixture-backed: source search parses official
  DIA reading-room page links, `get_record` accepts returned source IDs or
  official DIA URLs, and `list_assets` prefers PDF/FileId assets while retaining
  non-PDF official links as metadata leads.
- DIA keeps default source policy contracts:
  `cache_policy=RespectSourceHeaders` and `redirect_policy=Deny`. Citation
  notes warn that page citations require PDF ingestion and page-boundary
  verification.

Additional checkpoint after the OSD/Joint Staff FOIA Reading Room adapter slice:

- Added `src/sources/osd_joint_staff.rs` and split live helper modules for the
  official WHS/ESD OSD/Joint Staff FOIA Reading Room entry points at
  `https://www.esd.whs.mil/FOID/Reading-Room/` and
  `https://www.esd.whs.mil/Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/`.
- The adapter uses the source name `osd_joint_staff` for clarity in MCP
  validation, record IDs, and env overrides. Source search is conservative and
  fixture-backed: it parses the official category list plus the Joint Staff
  category listing, `get_record` accepts returned source IDs or official
  `www.esd.whs.mil` URLs, and `list_assets` prefers PDFs while retaining official
  HTML/non-PDF links as metadata leads.
- OSD/Joint Staff keeps default source policy contracts:
  `cache_policy=RespectSourceHeaders` and `redirect_policy=Deny`. Citation
  notes warn that page citations require PDF ingestion and page-boundary
  verification.

Current ingestion slices now include:

- Durable ingestion job lifecycle APIs with leases, stages, progress, warnings, terminal states, and resume-oriented tests.
- Startup/recovery is currently DB-centric: the worker reclaims queued/interrupted/expired-running jobs from SQLite, and resume tests cover stale page/chunk/FTS replacement. Derived `text/` and `ocr/` reconciliation APIs now exist internally, and SQLite FTS reconciliation/repair APIs now exist for explicit operator repair paths. Neither repair path is wired into startup or worker recovery.
- Source-record planning from normalized `SourceRecord` values into ingestion documents and selected assets.
- Bounded asset downloading into the content-addressed file store with cache provenance, ETag/Last-Modified revalidation, and `DoNotPersist` cache semantics.
- External `pdftotext` extraction with structured command arguments, timeout handling, bounded stderr capture, temp output validation, and text-quality warnings.
- `QueuedIngestionExecutor` claims durable jobs, resolves source records, plans ingestion, downloads selected assets, extracts text, persists pages/chunks, links assets, and records terminal job state.
- A single background ingestion worker is started by the Rust runtime. It opens a fresh SQLite handle per iteration, advances one queued job at a time, polls at a bounded interval when idle, and stops cleanly between iterations when the MCP service exits.
- Mid-job cancellation and shutdown interruption for queued ingestion. Active
  work is persisted as interrupted/resumable at safe boundaries, and cancellable
  local PDF/OCR child processes are killed without requiring live CIA/NARA HTTP
  or installed `pdftotext`/`ocrmypdf` tools in tests.
- Split download cache persistence boundaries so queued execution no longer
  holds the SQLite cache borrow across the async download await.
- Isolated source record resolution/asset listing awaits into a dedicated
  source-resolution boundary helper before synchronous planning.

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

Additional checkpoint after the FBI Vault adapter slice:

- Added a fixture-backed `fbi_vault` source adapter for official `vault.fbi.gov`
  search/file pages, keeping FBI Vault separate from DOJ Epstein and broader DOJ
  component FOIA adapters.
- The adapter preserves official Vault provenance, multipart part-order
  metadata, PDF-first asset ordering, citation/terms notes, and source warnings
  about historically uneven multipart files and page-boundary verification.
- FBI Vault remains on default source policy contracts:
  `cache_policy=RespectSourceHeaders` and `redirect_policy=Deny`; tests use
  deterministic loopback fixtures and do not require live network access.

Additional checkpoint after the derived-artifact reconciliation slice:

- Added internal derived-artifact reconciliation APIs for derived `text/` and
  `ocr/` files. `reconcile_derived_artifacts_for_document` reports
  `Missing`/`Stale`/`Orphaned` drift against SQLite page state,
  `plan_derived_artifact_repairs` turns that report into
  `RewriteFromSqlite`/`ManualReview` actions, and
  `apply_derived_artifact_repairs` performs opt-in writes for missing/stale
  derived artifacts from SQLite-derived content.
- SQLite remains canonical for normalized metadata, page text, chunks, job
  state, and FTS rows. Reconciliation does not mutate canonical DB rows or
  blobs, does not delete orphaned artifacts automatically, and is not wired
  into runtime/startup/worker auto-repair. Operator-facing MCP tools now expose
  report/plan/apply reconciliation, with apply gated by explicit confirmation.

Additional checkpoint after the SQLite FTS index repair slice:

- Added `src/index/reconcile_repair.rs` for explicit SQLite FTS repair planning
  and apply. Missing/stale `chunk_fts` rows are rewritten from canonical
  `documents`/`chunks` state in one transaction; orphaned FTS rows are
  conservative manual-review actions and are not deleted automatically.
- Focused tests cover missing, stale, duplicate, mixed, orphan-only,
  idempotent second apply, and canonical-table non-mutation behavior.

Additional checkpoint after the SQLite FTS MCP exposure slice:

- Exposed `report_sqlite_fts_drift`, `plan_sqlite_fts_repairs`, and
  `apply_sqlite_fts_repairs` as operator-facing MCP tools. Report/plan remain
  dry-run, apply requires the exact confirmation string surfaced by the plan,
  and orphaned `chunk_fts` rows stay manual-review only.
- README and eval guardrails now document the SQLite FTS report/plan/apply
  workflow alongside derived `text/`/`ocr/` artifact repair. No startup,
  worker, or runtime auto-repair path was added.

Additional checkpoint after the Army FOIA Reading Room adapter slice:

- Added `src/sources/army.rs` and split live helper modules for an official
  `army` adapter targeting `https://foia.army.mil/` and official
  `/Home/publicRecords/<category>` listing pages.
- The adapter is conservative and fixture-backed: source search parses official
  Army FOIA Reading Room table/document leads, `get_record` accepts returned
  `Home/DocContent/<id>` ids or official Army URLs, and `list_assets` prefers
  PDFs while retaining non-PDF document assets as metadata leads.
- Army keeps default source policy contracts:
  `cache_policy=RespectSourceHeaders` and `redirect_policy=Deny`.

Additional checkpoint after the Navy FOIA Reading Room family adapter slice:

- Added `src/sources/navy.rs` and split live helper modules for a conservative
  official `navy` adapter targeting the Department of the Navy
  `https://www.secnav.navy.mil/foia/readingroom/SitePages/Home.aspx` family,
  plus Naval Audit Service and Naval Inspector General reading-room pages under
  `secnav.navy.mil`.
- The adapter uses deterministic fixtures for search/list parsing. `get_record`
  accepts returned Navy source IDs or official same-origin SECNAV URLs, maps
  direct official PDFs without a live fetch, and `list_assets` prefers PDFs
  while retaining text/HTML/other assets as metadata leads.
- Navy keeps default source policy contracts:
  `cache_policy=RespectSourceHeaders` and `redirect_policy=Deny`.

Additional checkpoint after the Phase 9 indexing evaluation plan:

- Added `docs/phase9-indexing-evaluation.md` and `docs/next-dev-topics.md` to
  capture the decision rule for evaluating Tantivy before adding another local
  search backend. SQLite remains canonical, LanceDB remains deferred until a
  defined embedding model and measurable eval gain exist, and future index
  repair surfaces must stay explicit and operator-confirmed.

## Validation Commands

Run these before handing off or building the next slice:

```bash
just ai-gates
just fmt
just lint
just test
just architecture
git diff --check
```

Also keep running the final scans used for this batch: Rust LOC, production `unwrap|expect|panic|todo|unimplemented`, and outdated setup wording.

## First Action Next Session

Start with a read-only pass over the current repair surfaces before extending
them. Do not add automatic repair to startup or worker paths; keep repair
actions explicit, operator-confirmed, and covered by eval guardrails.

## Next Tasks

- Phase 8 source expansion is complete through the broader Navy reading-room
  family. If source expansion resumes, choose the next official source based on
  current research demand and source stability. Each
  source needs fixtures, source warning/citation notes, cache and redirect
  policy contracts, and at least one eval.
- Treat DOJ Epstein Library as a sensitive mixed-media source. Default to
  metadata/PDF ingestion, preserve DOJ privacy/victim-identification warnings in
  search and local retrieval output, and list images/audio/video without
  automatic ingestion until media safety rules are designed.
- Treat PURSUE/`war.gov` and AARO as separate UAP source candidates unless an
  official stable shared index appears. Prefer official release pages and linked
  assets over mirrors or news coverage.
- Add explicit redirect-follow policy only if a future source needs it; keep
  hop validation mandatory and preserve the current default-deny posture.
- Add a later `tesseract` backend only if a concrete source/OCR need appears;
  the backend config already has a small enum shape for another local backend.
- Derived-artifact and SQLite FTS repair are exposed through operator-facing
  MCP report/plan/apply tools. Keep runtime/startup/worker auto-repair wiring
  untouched, and keep future repair surfaces explicit, confirmed, and covered
  by guardrail evals.

## Constraints

- Do not grow existing oversized files such as `src/mcp/tools.rs`; add submodules for new behavior.
- Install hooks with `just install-hooks`; pre-commit runs `scripts/ai-dev-gates.sh --pre-commit`.
- Keep Rust modules under the gate limits. Existing oversized modules are frozen at their current line counts and must be split before adding behavior.
- Keep source IDs out of filesystem paths; use `document_key` and content hashes for local artifacts.
- Preserve source citation and terms notes with every ingested document.
- The TypeScript draft has been removed; Rust is the primary implementation target.
- Do not enable redirects by default; add explicit hop validation before allowing source-specific redirects.
- Do not shell out through a string command for PDF/OCR work; use structured `Command` arguments only.
