# foia-search

MCP server for declassified and FOIA document research.

## Design Direction

This repository is a Rust MCP server focused on PDF ingestion, local caching, OCR fallback, and searchable source-cited document text.

See [docs/foia-rust-design.md](docs/foia-rust-design.md) for the implementation design.

The Rust MCP server is the primary implementation. The old TypeScript draft has been removed.

## Rust MCP Tools

- `list_sources`: list configured FOIA/declassified-document sources, implementation status, and caveats.
- `search_source`: search one external source. AARO, Army FOIA Reading Room, CIA, GovInfo, PURSUE, DOJ Epstein, DOJ component FOIA, FBI Vault, FRUS, NOAA, NSA, State Department Virtual Reading Room, DIA FOIA Electronic Reading Room, OSD/Joint Staff FOIA Reading Room, and DTIC are wired; DTIC runs in accession/official-URL tracer mode with explicit fragility warnings, and NARA is wired when `FOIA_SEARCH_NARA_API_KEY` is configured.
- `get_source_record`: fetch one normalized source record by source ID or canonical URL. AARO, Army FOIA Reading Room, CIA, GovInfo, PURSUE, DOJ Epstein, DOJ component FOIA, FBI Vault, FRUS, NOAA, NSA, State Department Virtual Reading Room, DIA FOIA Electronic Reading Room, OSD/Joint Staff FOIA Reading Room, DTIC, and configured NARA are wired.
- `ingest_document`: create a durable queued ingestion job for a source-prefixed document ID such as `cia:CREST-...`.
- `get_ingestion_job`: read durable ingestion job status, progress, errors, and next actions.
- `search_local_documents`: search locally ingested metadata/page/chunk text through the SQLite FTS index.
- `get_document`: return normalized metadata and provenance for an ingested local document.
- `get_document_text`: return extracted/OCR text for an explicit one-based page range of at most 50 pages.
- `refresh_document`: create a queued refresh job for a local/source-prefixed document.
- `report_derived_artifact_drift`: report drift in derived `text/` and `ocr/` artifacts without writing.
- `plan_derived_artifact_repairs`: plan missing/stale rewrites and orphan manual-review actions without writing.
- `apply_derived_artifact_repairs`: apply missing/stale derived-artifact rewrites from SQLite after explicit confirmation; orphaned artifacts remain manual-review only.
- `report_sqlite_fts_drift`: report drift between canonical SQLite `chunks` and derived `chunk_fts` rows without writing.
- `plan_sqlite_fts_repairs`: plan safe `chunk_fts` rewrites and orphan manual-review actions without writing.
- `apply_sqlite_fts_repairs`: apply missing/stale `chunk_fts` rewrites from canonical SQLite chunks after explicit confirmation; orphan rows remain manual-review only.

Derived `text/`/`ocr/` artifact reconciliation and SQLite FTS index reconciliation are exposed through explicit operator-facing MCP report/plan/apply tools. The runtime does not auto-repair those artifacts on startup or in background workers.

## Rust Run

```bash
cargo build
cargo run
```

Common project gates:

```bash
just fmt
just lint
just test
just architecture
just ai-gates
```

Install the repo pre-commit hook before development:

```bash
just install-hooks
```

The pre-commit hook runs AI-development gates that keep Rust modules from
growing unchecked, block unchecked production `unwrap`/`expect`/panic-style
calls, reject staged generated outputs, and run Rust format/lint checks.

Rust MCP config example:

```json
{
  "mcpServers": {
    "foia-search": {
      "command": "cargo",
      "args": ["run", "--manifest-path", "/path/to/foia-search/Cargo.toml"],
      "env": {
        "FOIA_SEARCH_DATA_DIR": "/path/to/foia-search-data"
      }
    }
  }
}
```

For a compiled binary, set `command` to the built executable path and omit the Cargo arguments.

## Env

- `FOIA_SEARCH_DATA_DIR`: local cache/index directory. Defaults to a platform-specific data directory.
- `FOIA_SEARCH_CIA_BASE_URL`: defaults to `https://www.cia.gov`.
- `FOIA_SEARCH_NARA_API_KEY`: required for NARA adapter requests.
- `FOIA_SEARCH_NARA_API_BASE_URL`: defaults to `https://catalog.archives.gov/api/v2`.
- `FOIA_SEARCH_GOVINFO_API_KEY`: optional GovInfo API key. Defaults to `DEMO_KEY`.
- `FOIA_SEARCH_GOVINFO_API_BASE_URL`: defaults to `https://api.govinfo.gov`.
- `FOIA_SEARCH_FBI_VAULT_BASE_URL`: defaults to `https://vault.fbi.gov`.
- `FOIA_SEARCH_FRUS_BASE_URL`: defaults to `https://history.state.gov`.
- `FOIA_SEARCH_DTIC_BASE_URL`: defaults to `https://apps.dtic.mil`.
- `FOIA_SEARCH_NOAA_BASE_URL`: defaults to `https://repository.library.noaa.gov`.
- `FOIA_SEARCH_NSA_BASE_URL`: defaults to `https://www.nsa.gov`.
- `FOIA_SEARCH_STATE_BASE_URL`: defaults to `https://foia.state.gov`.
- `FOIA_SEARCH_DIA_BASE_URL`: defaults to `https://www.dia.mil`.
- `FOIA_SEARCH_OSD_JOINT_STAFF_BASE_URL`: defaults to `https://www.esd.whs.mil`.
- `FOIA_SEARCH_ARMY_BASE_URL`: defaults to `https://foia.army.mil`.
- `FOIA_SEARCH_OCR_FALLBACK`: optional local OCR fallback policy. Defaults to `off`;
  set to `on_quality_warning` to allow local OCR when embedded PDF text has
  quality warnings or embedded extraction fails.
- `FOIA_SEARCH_OCR_BACKEND`: optional local OCR backend. Defaults to `none`;
  set to `ocrmypdf` to run the `ocrmypdf` backend. OCR still remains disabled
  unless `FOIA_SEARCH_OCR_FALLBACK=on_quality_warning` is also set.
- `FOIA_SEARCH_OCRMYPDF_BIN`: optional `ocrmypdf` executable path. Defaults to
  `ocrmypdf`.
- `FOIA_SEARCH_OCR_TIMEOUT_SECONDS`: optional local OCR command timeout.
  Defaults to `300`.
- `FOIA_SEARCH_OCR_MAX_STDERR_BYTES`: optional stderr capture limit for local
  OCR command failures. Defaults to `8192`.

## Notes

CIA OCR and HTML are uneven. Treat tool output as a lead finder, then cite the original scan/PDF.
NARA API behavior changes over time. Use `list_sources` for source status and manual source
guidance when the adapter is disabled, unavailable, or not configured.
