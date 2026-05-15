# FOIA Search Rust Design

## Reader And Goal

This document is for an engineer implementing the next version of the FOIA Search MCP server. After reading it, they should be able to scaffold a new Rust crate, port only the useful pieces from the TypeScript draft, and implement PDF ingestion, caching, local indexing, source adapters, and MCP tools in a phased way.

The current TypeScript server should be treated as a prototype for source behavior and tool naming, not as the implementation base. The production direction is a new Rust crate with selective reuse from the existing Rust paper-search server.

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

Replace the TypeScript draft during implementation. It can remain useful as a reference while porting CIA and NARA behavior, but the Rust crate should become the canonical server and runtime.

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
    documents/<document_id>.txt
    pages/<document_id>/<page_number>.txt
  ocr/
    pages/<document_id>/<page_number>.txt
  index/
    sqlite/
    tantivy/
    lance/
  tmp/
  logs/
    ingestion.jsonl
```

The file store should be content-addressed by SHA-256. Database rows should point to blob paths and preserve original source URLs, content type, ETag, Last-Modified, fetch time, and source policy notes.

## Metadata Model

Use one normalized document model across sources.

Core fields:

- `id`: local stable ID, usually `<source>:<source_id>`.
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

## Source Plan

CIA Reading Room is the first adapter. Port the TypeScript behavior: search public Reading Room HTML, parse document pages, find PDF attachments, and warn when page structure looks wrong. It is valuable as a real-world scraper test because the HTML is not a clean API.

NARA Catalog is second. Add API-key configuration, source-specific rate budgeting, and support for records, digital objects, and extracted text when present. Do not bulk mirror NARA metadata. Disable persistent NARA API response caching by default; retain only the normalized metadata needed for a user-requested ingested document unless a separately reviewed data export path is implemented.

GovInfo is third. Use official API search and package/granule concepts. This is best for Congressional Record, hearings, statutes, CFR, reports, and government publications. Prefer API PDF/XML/MODS links over HTML scraping.

FRUS is fourth. Use the Office of the Historian catalog/API, TEI/XML where available, and PDF/ebook links for volumes. FRUS should support document-level citation better than most sources.

NOAA is fifth. Target the NOAA Institutional Repository and related technical report collections. Prefer repository metadata and document PDFs. Treat OAI-PMH or repository JSON as a source-specific implementation detail behind the adapter.

DTIC is sixth. Treat public DTIC as a fragile adapter until a stable official API is confirmed. Do not depend on undocumented endpoints for core functionality without clear warnings and tests.

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

Blob files, temp files, OCR outputs, and future Tantivy/LanceDB index writes cannot be committed atomically with SQLite. The job table should therefore store stage checkpoints and an outbox of pending file/index work. On startup and before each job resume, reconcile the database with the file store and indexes so partial writes can be detected, retried, or discarded.

Extraction quality should consider empty pages, very low character count, replacement characters, repeated garbage, and whether extracted pages match the PDF page count.

Page citations are part of the contract. Use `pdfinfo` or equivalent to capture source page count, then preserve page boundaries by extracting one page at a time or by verifying form-feed-delimited `pdftotext` output against that page count. If page boundaries cannot be verified, index the text but mark page citations as uncertain instead of returning confident page numbers.

OCR should be opt-in by configuration and tool parameter. It is expensive and has more local dependency requirements than embedded text extraction.

## Direct Input Safety

Direct URL and local-file ingestion should be disabled by default for model callers. The normal path is source-mediated ingestion: search a known adapter, fetch the source record, list assets, then ingest a source-approved asset URL.

If direct URL ingestion is enabled, enforce:

- `https` only unless explicitly configured otherwise.
- Source or host allowlists.
- Redirect limits and redirect target revalidation.
- DNS resolution checks that block private, loopback, link-local, multicast, and metadata-service address ranges.
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
- `search_sources`: search one or more official sources for candidate records.
- `get_source_record`: fetch normalized metadata and assets for one source record.
- `ingest_document`: ingest by source/id or, when explicitly enabled, a validated URL/local file; return an ingestion job.
- `get_ingestion_job`: inspect job status, progress, warnings, and next action.
- `search_local_documents`: search locally indexed text with filters and page-cited snippets.
- `get_document`: return cached metadata, assets, extraction status, and citation note.
- `get_document_text`: return text by page range or chunk IDs.
- `refresh_document`: revalidate metadata/assets and optionally re-extract.

Tool descriptions must tell the model when to use the tool, when not to use it, and what to do with errors. Outputs should be compact, JSON-shaped, and decision-ready. Full document text should never be returned by default.

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

- Adapter parsing fixtures for CIA, NARA, GovInfo, FRUS, NOAA, and DTIC.
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

Create the Rust crate, MCP stdio server, config, logging, source registry, error types, and placeholder tools. Port no ingestion behavior yet. Confirm the server works under MCP Inspector.

### Phase 2: Storage And Jobs

Add data directory handling, SQLite schema, migrations, content-addressed file store, fetch cache, and ingestion job tables. Implement job creation and status inspection.

### Phase 3: PDF Text MVP

Implement direct PDF ingestion only behind explicit configuration for trusted URLs or confined local import directories. Download, hash, store, extract text with `pdftotext`, verify page boundaries, chunk, and index with SQLite FTS5.

### Phase 4: Local Search

Implement `search_local_documents`, `get_document`, and `get_document_text`. Results must include snippets, document metadata, source URL, and page ranges.

### Phase 5: CIA Adapter

Port CIA search/detail parsing from the TypeScript prototype into Rust. Add ingest-by-CIA-ID and fixtures for search and document pages.

### Phase 6: NARA Adapter

Add NARA API-key config, search, record fetch, digital-object extraction, source OCR text when available, and conservative cache policy.

### Phase 7: OCR Fallback

Add optional OCR dependency detection, extraction-quality scoring, page-level OCR status, and re-ingestion support.

### Phase 8: Official Source Expansion

Add GovInfo, FRUS, NOAA, and DTIC in that order. Each adapter must ship with fixtures, rate/cache policy, and at least one eval.

### Phase 9: Advanced Indexing

Evaluate Tantivy and LanceDB reuse from paper-search. Move to Tantivy when FTS5 ranking or corpus size becomes limiting. Add LanceDB only with a defined embedding model and measurable eval improvement.

## Open Questions

- Should OCR be enabled by default when dependencies are installed, or only when a tool call requests it?
- Should first-version ingestion support local file paths, remote URLs, or both?
- What is the acceptable cache policy for NARA-derived metadata in this project?
- Should semantic/vector search be deferred until after GovInfo/FRUS/NOAA adapters are implemented?

## Reference Notes

- NARA Catalog API documentation describes API-key access, rate limits, extracted text support, and cache/scrape cautions: <https://www.archives.gov/research/catalog/help/api>
- GovInfo documents API search, packages, granules, and PDF/XML/MODS links: <https://www.govinfo.gov/features/search-service-overview>
- Office of the Historian documents the FRUS catalog/API and XML/TEI-backed publication model: <https://history.state.gov/developer>
- NOAA Institutional Repository is the target entry point for NOAA-authored or NOAA-funded publications and reports: <https://repository.library.noaa.gov/>
