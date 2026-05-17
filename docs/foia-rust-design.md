# FOIA Search Rust Design

## Reader And Goal

This document is for an engineer implementing the Rust FOIA Search MCP server. After reading it, they should be able to understand the current crate and implement PDF ingestion, caching, local indexing, source adapters, and MCP tools in a phased way.

The removed TypeScript server was a prototype for source behavior and tool naming, not the implementation base. The production direction is the Rust crate with selective reuse from the existing Rust paper-search server.

## Decision Summary

Build a new Rust MCP server for FOIA and declassified-document research.

Use paper-search as the local pattern for:

- `rmcp`-based MCP server structure.
- `schemars` parameter schemas.
- Async source traits.
- Environment-driven configuration.
- Local indexing concepts.
- Tantivy/LanceDB modules where they are still a good fit.

Do not make FOIA Search a TypeScript server. PDF ingestion, OCR orchestration, hashing, durable job state, concurrent downloads, and local indexing are all better suited to Rust. A single compiled MCP binary is also simpler to run and distribute than a Node toolchain with native PDF/OCR dependencies.

Keep FOIA Search separate from paper-search at first. The document model, citation requirements, page-level text, and source terms are different enough that merging too early would force awkward abstractions. Reuse code or design patterns selectively after the FOIA shape is clear.

The TypeScript draft has been replaced. The Rust crate is the canonical server and runtime.

## Design Constraints

The server must help a model complete research tasks, not expose raw source APIs. Tools should be few, task-shaped, and explicit about when to use them.

Every local search result must be traceable to a source document and preferably to a page range. The system should preserve original PDFs, extracted text, OCR text, metadata, fetch provenance, and warnings separately.

The ingestion pipeline must be resumable. Long-running PDF download, extraction, and OCR should be represented as jobs with stable IDs, status, progress, errors, and next actions.

The system must respect source-specific terms, rate limits, and cache restrictions. In particular, NARA's Catalog API requires an API key for normal use, documents a default monthly query limit, and cautions against broad caching or scraping. The design should support source-specific cache policies instead of assuming every public document source can be mirrored freely.

## Crate Shape

Start with one Rust crate named `foia-search` or `foia-search-rs`.

Recommended module layout:

```text
src/
  main.rs
  config.rs
  errors.rs
  mcp/
    mod.rs
    tools.rs
    output.rs
  sources/
    mod.rs
    cia.rs
    nara.rs
    govinfo.rs
    frus.rs
    dtic.rs
    noaa.rs
  ingest/
    mod.rs
    jobs.rs
    pipeline.rs
    pdf.rs
    ocr.rs
    chunk.rs
  store/
    mod.rs
    sqlite.rs
    cache.rs
    files.rs
  index/
    mod.rs
    sqlite_fts.rs
    tantivy.rs
    hybrid.rs
  model.rs
```

The crate should compile and run as an MCP stdio server from the start, even before every source adapter is implemented.

## Dependency Direction

Use the same broad Rust ecosystem as paper-search:

- MCP: `rmcp`
- Runtime: `tokio`
- HTTP: `reqwest`
- Serialization: `serde`, `serde_json`
- Tool schemas: `schemars`
- Errors: `thiserror`, `anyhow` where appropriate
- HTML parsing: `scraper`
- XML parsing: `quick-xml`
- Logging: `tracing`, `tracing-subscriber`
- SQLite: `rusqlite` with bundled SQLite/FTS5 support for the first version
- Migrations: checked-in SQL migrations run synchronously during startup before serving tools
- Full-text index: SQLite FTS5 for MVP, Tantivy as the scale path
- Vector index: LanceDB later, not in the first ingestion milestone
- PDF extraction: external `pdftotext` first
- OCR fallback: optional external `ocrmypdf` or `tesseract`

Prefer external PDF/OCR binaries for the first version. They are mature, easy to replace, and keep PDF parsing complexity out of the MCP server.

Keep SQLite access behind a small synchronous store layer. MCP tool handlers can call it through `tokio::task::spawn_blocking` where needed. Avoid adopting an async SQL abstraction until there is a real concurrent-write requirement.

## Data Directory

Use `FOIA_SEARCH_DATA_DIR`, defaulting to a user-local directory.

```text
$FOIA_SEARCH_DATA_DIR/
  db/
    foia.sqlite
  blobs/
    pdf/
      sha256/<hash>.pdf
    html/
      sha256/<hash>.html
    other/
      sha256/<hash>
  text/
    documents/<document_key>.txt
    pages/<document_key>/<page_number>.txt
  ocr/
    pages/<document_key>/<page_number>.txt
  index/
    sqlite/
    tantivy/
    lance/
  tmp/
  logs/
    ingestion.jsonl
```

The file store should be content-addressed by SHA-256. Database rows should point to blob paths and preserve original source URLs, content type, ETag, Last-Modified, fetch time, and source policy notes.

SQLite is the canonical store for normalized metadata, page text, chunks, job state, and FTS rows. Files under `text/` and `ocr/` are derived audit/debug artifacts only; the Rust server now has internal report/plan/apply reconciliation for those derived artifacts against SQLite page state. Report identifies `Missing`/`Stale`/`Orphaned` drift, plan maps that into `RewriteFromSqlite` and `ManualReview` actions, and apply writes only missing/stale derived artifacts from SQLite-derived content. Original PDFs, HTML, and other fetched assets remain canonical content-addressed blobs.

Never use source IDs directly in filesystem paths. Every document should get a filesystem-safe internal `document_key`, such as a generated UUID or stable hash over source plus source ID. Store external source IDs and URLs separately in SQLite.

## Metadata Model

Use one normalized document model across sources.

Core fields:

- `id`: user-facing stable ID, usually `<source>:<source_id>`, never used directly as a file path.
- `document_key`: filesystem-safe internal key used for derived text/OCR paths.
- `source`: `cia`, `nara`, `govinfo`, `frus`, `dtic`, `noaa`, or future adapter name.
- `source_id`: ID from the original source.
- `title`
- `date`
- `collection`
- `record_group`
- `description`
- `origin_url`
- `document_url`
- `pdf_url`
- `metadata_json`
- `citation_note`
- `terms_note`
- `created_at`
- `updated_at`

Asset fields:

- `document_id`
- `asset_url`
- `mime_type`
- `role`: `pdf`, `html`, `ocr_text`, `transcript`, `image`, `other`
- `sha256`
- `size_bytes`
- `etag`
- `last_modified`
- `fetched_at`
- `cache_policy`

Text fields:

- `document_id`
- `page_number`
- `text`
- `source`: `embedded_pdf_text`, `source_ocr`, `local_ocr`, `html`, `tei`, `api_text`
- `quality_score`
- `warnings`

Chunk fields:

- `document_id`
- `chunk_id`
- `page_start`
- `page_end`
- `text`
- `token_estimate`
- `metadata_json`

Keep raw metadata and normalized fields. FOIA sources are uneven; the raw record is often needed for later adapter fixes.

Current ingestion planning stores full source metadata under `source_metadata` in document
`metadata_json`, alongside an `ingest_plan` object describing the selected asset, cache policy,
and source metadata keys.

## Ingestion Lifecycle

Queued ingestion jobs now support explicit claim leases, monotonic progress, stage updates,
deduplicated warnings, error recording, completion, failure, and interruption. Resume logic should
claim queued, interrupted, or expired running jobs and continue from the stored stage/progress.

Asset downloads write successful bodies to the content-addressed file store, record cache
provenance when source policy allows persistence, and revalidate with ETag/Last-Modified. The
default downloader rejects redirects instead of following them implicitly; any future redirect
support must validate each hop before fetching.

PDF text extraction can use an external `pdftotext` binary with structured arguments, bounded
stderr, temp output validation, timeout handling, and quality warnings for blank or low-density
embedded text.

## Source Adapter Interface

Each source adapter should implement a common async trait:

```rust
#[async_trait]
pub trait SourceAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn status(&self) -> SourceStatus;

    async fn search(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<SearchPage, SourceError>;

    async fn get_record(&self, id_or_url: &str) -> Result<SourceRecord, SourceError>;

    async fn list_assets(&self, record: &SourceRecord) -> Result<Vec<SourceAsset>, SourceError>;

    fn cache_policy(&self) -> CachePolicy;
}
```

Search should return normalized records and an opaque cursor. Do not expose source page numbers as MCP pagination unless the source only supports page numbers internally. The adapter should hide that and return a cursor the model can pass back.

For the MVP, source search should paginate one source at a time and the MCP schema should require exactly one `source`. If federated multi-source search is added later, its opaque cursor must encode per-source cursors, exhausted-source flags, query/options hash, and merge/ranking state so subsequent calls cannot mix incompatible searches.

## Source Plan

CIA Reading Room is the first adapter. It should search public Reading Room HTML, parse document pages, find PDF attachments, and warn when page structure looks wrong. It is valuable as a real-world scraper test because the HTML is not a clean API.

NARA Catalog is second. Add API-key configuration, source-specific rate budgeting, and support for records, digital objects, and extracted text when present. Do not bulk mirror NARA metadata. Disable persistent NARA API response caching by default; retain only the normalized metadata needed for a user-requested ingested document unless a separately reviewed data export path is implemented.

GovInfo is third. Use official API search and package/granule concepts. This is best for Congressional Record, hearings, statutes, CFR, reports, and government publications. Prefer API PDF/XML/MODS links over HTML scraping.

FRUS is fourth. Use the Office of the Historian catalog/API, TEI/XML where available, and PDF/ebook links for volumes. FRUS should support document-level citation better than most sources.

NOAA is fifth. Target the NOAA Institutional Repository and related technical report collections. Prefer repository metadata and document PDFs. Treat OAI-PMH or repository JSON as a source-specific implementation detail behind the adapter.

DTIC is sixth. Treat public DTIC as a fragile adapter until a stable official API is confirmed. Do not depend on undocumented endpoints for core functionality without clear warnings and tests.

PURSUE / War Department UAP releases should be added as an official-source adapter after GovInfo or alongside the UAP-specific source batch. Target `https://www.war.gov/ufo/` and official linked release assets first. Preserve release tranche metadata, list PDFs/images/videos as assets, ingest PDFs by default, and leave images/videos metadata-only until media handling is explicitly designed.

AARO UAP records should be tracked as a related official UAP source. Prefer official AARO historical-record and release pages over mirrors, preserve originating agency/release notes, and keep the adapter separate from PURSUE unless the official sites expose one stable shared index.

DOJ Epstein Library should be a high-priority official source because `https://www.justice.gov/epstein` and `https://www.justice.gov/epstein/doj-disclosures` centralize materials released under the Epstein Files Transparency Act. Treat this source as sensitive: surface DOJ privacy/victim-identification warnings in source results, default to metadata/PDF ingestion only, list images/audio/video as assets without automatic ingestion, and preserve data-set/court-record/FOIA category provenance.

DOJ component disclosure libraries should become a broader `doj_foia` adapter family. Start from the DOJ Office of Information Policy's "Available Documents for All DOJ Components" index, then add component-specific sub-adapters only where the source exposes stable official listing pages or APIs.

FBI Vault should be a separate adapter from DOJ Epstein and DOJ OIP. Target official Vault search/listing pages, proactive disclosures, discretionary releases, and PDF file pages; keep citation/source notes explicit because Vault files may be multipart and historically uneven.

## Ingestion Pipeline

The ingestion pipeline should be explicit and resumable:

1. Resolve source record from source/id/url.
2. List candidate assets.
3. Pick an ingestible asset, usually PDF first.
4. Fetch asset with cache and rate-limit policy.
5. Hash and store blob.
6. Extract embedded PDF text.
7. Score extraction quality.
8. Run OCR fallback if text is missing or low quality.
9. Normalize text while preserving page boundaries.
10. Chunk page-aware text.
11. Write metadata, pages, chunks, and SQLite FTS entries in one database transaction.
12. Return job status, warnings, citations, and next actions.

Blob files, temp files, OCR outputs, and future Tantivy/LanceDB index writes cannot be committed atomically with SQLite. The job table should therefore store stage checkpoints and an outbox of pending file/index work. Current startup and recovery behavior is DB-centric: queued, interrupted, and expired-running jobs are claimed from SQLite, and resume paths replace stale page/chunk/FTS rows as part of that same database workflow. Internal report/plan/apply APIs now cover derived `text/` and `ocr/` reconciliation, but they are not invoked automatically by startup or worker recovery. Future index-artifact reconciliation remains planned follow-on work, where partial writes can eventually be detected, retried, or discarded without changing the current SQLite-first recovery path.

Extraction quality should consider empty pages, very low character count, replacement characters, repeated garbage, and whether extracted pages match the PDF page count.

Page citations are part of the contract. Use `pdfinfo` or equivalent to capture source page count, then preserve page boundaries by extracting one page at a time or by verifying form-feed-delimited `pdftotext` output against that page count. If page boundaries cannot be verified, index the text but mark page citations as uncertain instead of returning confident page numbers.

OCR should be opt-in by configuration and tool parameter. It is expensive and has more local dependency requirements than embedded text extraction.

External process execution is part of the ingestion threat model. PDF/OCR commands must be launched with structured `Command` arguments, never through a shell. Configure or discover fixed binary paths at startup, run commands in confined temp directories, set per-process timeouts, kill process groups on timeout/cancel, cap stdout/stderr/output file sizes, validate expected output paths, and clean temp files on both success and failure.

## Direct Input Safety

Direct URL and local-file ingestion should be disabled by default for model callers. The normal path is source-mediated ingestion: search a known adapter, fetch the source record, list assets, then ingest a source-approved asset URL.

If direct URL ingestion is enabled, enforce:

- `https` only unless explicitly configured otherwise.
- Source or host allowlists.
- Redirect limits and redirect target revalidation.
- A controlled download path that resolves, validates, and pins candidate IPs for the actual connection. Revalidate after every redirect, preserve correct Host/SNI behavior, and reject any final peer address in private, loopback, link-local, multicast, or metadata-service address ranges. Do not rely on a separate preflight DNS check followed by an unconstrained HTTP client request.
- Maximum response size and maximum PDF page count.
- Content-Type and file-signature validation.
- Download timeouts and per-host concurrency limits.

If local-file ingestion is enabled, confine paths to configured import directories. Resolve symlinks before validation and reject paths outside those directories. Never allow arbitrary absolute paths from an MCP caller.

## Indexing Choice

Use SQLite plus FTS5 for the first implementation.

Reasons:

- The metadata, cache, job state, and FTS index can live in one database.
- It is simple to test and inspect.
- It is enough for local FOIA corpora while the ingestion model stabilizes.
- It avoids copying too much of paper-search before the FOIA document model is settled.

Keep the index abstraction narrow so Tantivy can replace or supplement FTS5 later. The search interface should return scored chunk hits with document metadata and page ranges, not raw index rows.

Add Tantivy after the first useful corpus exists and SQLite ranking becomes a constraint. Add LanceDB only after there is a clear semantic-search requirement and an embedding strategy for historical/government documents.

## MCP Tool Design

Expose task-shaped tools, not raw adapter endpoints.

Initial tools:

- `list_sources`: show enabled sources, auth status, rate-limit notes, and cache policy notes.
- `search_source`: search one official source for candidate records. Multi-source search is deferred until federated cursor state is designed.
- `get_source_record`: fetch normalized metadata and assets for one source record.
- `ingest_document`: ingest by source/id or, when explicitly enabled, a validated URL/local file; return an ingestion job.
- `get_ingestion_job`: inspect job status, progress, warnings, and next action.
- `search_local_documents`: search locally indexed text with filters and page-cited snippets.
- `get_document`: return cached metadata, assets, extraction status, and citation note.
- `get_document_text`: return text by page range or chunk IDs.
- `refresh_document`: revalidate metadata/assets and optionally re-extract.

Tool descriptions must tell the model when to use the tool, when not to use it, and what to do with errors. Outputs should be compact, JSON-shaped, and decision-ready. Full document text should never be returned by default.

`ingest_document` should enqueue durable work and return quickly with a job ID. A bounded background worker pool owns ingestion execution, with per-source concurrency limits and a process-wide cap for OCR/PDF subprocesses. Jobs need leases or locks so only one worker advances a job at a time. On shutdown, workers should stop accepting new work, let short stages finish, mark interrupted jobs resumable, and release leases. On startup, the server should recover queued/interrupted jobs before accepting new ingestion work. Derived `text/` and `ocr/` reconciliation is available only through explicit internal report/plan/apply APIs for now; it is not part of startup recovery. Broader file-store/index reconciliation for future index artifacts remains a planned follow-on step, not a current startup requirement.

## Error And Output Rules

Translate source and ingestion failures into actionable MCP errors:

- Rate limit: include source, retry guidance, and whether cached data is available.
- Auth missing: name the required environment variable.
- No PDF found: return candidate non-PDF assets and source URL.
- OCR unavailable: name the missing binary and show how to continue without OCR.
- Extraction low quality: return page counts, text quality score, and OCR recommendation.
- Source changed: return adapter warning and manual source URL.

Do not return raw HTTP errors or stack traces to MCP callers.

## Rate Limits And Caching

Implement per-source rate policies:

- Request timeout.
- Max concurrent requests.
- Minimum delay between requests.
- Retry count.
- Backoff strategy.
- Monthly or daily quota where known.
- Whether API responses may be cached and for how long.
- Whether downloaded public assets may be retained.

Use conditional GET where supported and allowed by the source policy. Store ETag and Last-Modified for retained assets and cacheable responses. Keep cache provenance in the database so a caller can see whether a result is live, cached, stale, unavailable due to rate limits, or not retained because of source terms.

Default behavior should be polite and bounded. Bulk ingestion should require explicit max-document limits and source filters.

## Testing Strategy

Start with deterministic fixture tests:

- Adapter parsing fixtures for CIA, NARA, GovInfo, PURSUE/war.gov UAP, AARO, DOJ Epstein Library, DOJ component FOIA indexes, FBI Vault, FRUS, NOAA, and DTIC.
- Cursor encoding and decoding.
- Cache hit, stale hit, conditional GET, and 304 behavior.
- PDF text extraction fixtures.
- Scanned PDF OCR fallback fixtures.
- Chunk page mapping.
- SQLite schema migrations.
- FTS search ranking and filters.
- MCP parameter validation.

Add integration tests that use a temp data directory and fixture HTTP server. The tests should prove jobs can resume after partial failure.

Use MCP Inspector before declaring the server usable. Every tool should register, schema-validate, return compact output, and produce helpful errors.

## Evaluation Set

Maintain `evals.xml` with at least 10 model-facing tasks. Each eval should require two or more tool calls and a decision based on previous output.

Coverage:

- Search CIA, choose a record, ingest its PDF, search local text, and cite a page.
- Search NARA with and without available-online filters, then ingest a record with digital objects.
- Use GovInfo for a congressional hearing and retrieve a cited snippet.
- Use PURSUE/war.gov UAP releases to find a release-tranche document and ingest an official PDF.
- Use DOJ Epstein Library to find an EFTA data-set PDF while preserving DOJ sensitivity warnings.
- Use FBI Vault to retrieve a multipart FOIA file with correct source provenance.
- Use FRUS for a policy document and return source citation metadata.
- Handle a scanned PDF that requires OCR.
- Continue paginated source search using an opaque cursor.
- Recover from a rate-limit response using cached data.
- Handle a record with metadata but no downloadable asset.
- Refresh a stale cached document.
- Compare local results across CIA/NARA/GovInfo for one query.

The target is at least 8 of 10 successful model completions without handholding.

## Phased Milestones

### Phase 1: Rust Scaffold

Create the Rust crate, MCP stdio server, config, logging, source registry, error types, and initial tool registrations. Port no ingestion behavior yet. Confirm the server works under MCP Inspector.

### Phase 2: Storage And Jobs

Add data directory handling, SQLite schema, migrations, content-addressed file store, fetch cache, and ingestion job tables. Implement job creation and status inspection.

### Phase 3: PDF Text MVP

Implement direct PDF ingestion only behind explicit configuration for trusted URLs or confined local import directories. Download, hash, store, extract text with `pdftotext`, verify page boundaries, chunk, and index with SQLite FTS5.

### Phase 4: Local Search

Implement `search_local_documents`, `get_document`, and `get_document_text`. Results must include snippets, document metadata, source URL, and page ranges.

### Phase 5: CIA Adapter

Implement CIA search/detail parsing in Rust. Add ingest-by-CIA-ID and fixtures for search and document pages.

### Phase 6: NARA Adapter

Add NARA API-key config, search, record fetch, digital-object extraction, source OCR text when available, and conservative cache policy.

### Phase 7: OCR Fallback

Add optional OCR dependency detection, extraction-quality scoring, page-level OCR status, and re-ingestion support.

### Phase 8: Official Source Expansion

Add as many official sources as practical while keeping each adapter policy-pinned and fixture-backed. Current priority order:

1. GovInfo live API adapter.
2. PURSUE / `war.gov` UAP release adapter.
3. DOJ Epstein Library adapter.
4. DOJ component FOIA/disclosure index adapter family.
5. FBI Vault adapter.
6. AARO UAP historical records adapter.
7. FRUS.
8. NOAA Institutional Repository.
9. DTIC.

Each adapter must ship with fixtures, rate/cache policy, redirect policy, source warning/citation notes, and at least one eval. Sensitive mixed-media collections such as DOJ Epstein should default to metadata/PDF ingestion and list images/audio/video without automatic ingestion until explicit media safety rules exist.

### Phase 9: Advanced Indexing

Evaluate Tantivy and LanceDB reuse from paper-search. Move to Tantivy when FTS5 ranking or corpus size becomes limiting. Add LanceDB only with a defined embedding model and measurable eval improvement.

## Open Questions

- Which exact OCR modes should be exposed once OCR is explicitly enabled: whole-document OCR, page-level OCR, or only low-quality-page fallback?
- Which direct-ingestion allowlists should ship as examples for trusted local development without weakening the default source-mediated path?
- What is the acceptable cache policy for NARA-derived metadata in this project?
- Should semantic/vector search be deferred until after GovInfo/FRUS/NOAA adapters are implemented?
- What source-warning contract should sensitive collections such as DOJ Epstein use so privacy/victim-identification cautions persist through search, ingestion, and local retrieval?
- What additional guardrails and evals should ship for the exposed operator-facing MCP report/plan/apply reconciliation surface?

## Reference Notes

- NARA Catalog API documentation describes API-key access, rate limits, extracted text support, and cache/scrape cautions: <https://www.archives.gov/research/catalog/help/api>
- GovInfo documents API search, packages, granules, and PDF/XML/MODS links: <https://www.govinfo.gov/features/search-service-overview>
- PURSUE / War Department UAP releases: <https://www.war.gov/ufo/>
- DOJ Epstein Library: <https://www.justice.gov/epstein>
- DOJ Epstein disclosures: <https://www.justice.gov/epstein/doj-disclosures>
- DOJ component disclosure index: <https://www.justice.gov/oip/available-documents-all-doj-components>
- FBI Vault: <https://vault.fbi.gov/>
- Office of the Historian documents the FRUS catalog/API and XML/TEI-backed publication model: <https://history.state.gov/developer>
- NOAA Institutional Repository is the target entry point for NOAA-authored or NOAA-funded publications and reports: <https://repository.library.noaa.gov/>
