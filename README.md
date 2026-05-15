# foia-search

MCP server for declassified and FOIA document research.

## Design Direction

This repository is moving from the TypeScript draft toward a new Rust MCP server focused on PDF ingestion, local caching, OCR fallback, and searchable source-cited document text.

See [docs/foia-rust-design.md](docs/foia-rust-design.md) for the implementation design.

The sections below document the current TypeScript draft only. The Rust rewrite is expected to be a breaking replacement, with compatibility aliases considered only if they prove useful during migration.

## Tools

- `search_cia_reading_room`: search CIA FOIA Electronic Reading Room.
- `get_cia_document`: fetch one CIA document page and exposed scan/PDF links.
- `search_nara_catalog`: search National Archives Catalog API.
- `get_nara_record`: fetch one NARA record by NAID.
- `search_official_declass_sources`: return official source entry points and caveats.

## Run

```bash
npm install
npm run build
node dist/index.js
```

## MCP Config

```json
{
  "mcpServers": {
    "foia-search": {
      "command": "/home/blaine/projects/ResearchTools/foia-search/dist/index.js",
      "env": {}
    }
  }
}
```

## Env

- `FOIA_SEARCH_CIA_BASE_URL`: defaults to `https://www.cia.gov`
- `FOIA_SEARCH_NARA_API_BASE_URL`: defaults to `https://catalog.archives.gov/api/v2/records`
- `FOIA_SEARCH_MAX_RESULTS`: defaults to `25`

## Notes

CIA OCR and HTML are uneven. Treat tool output as a lead finder, then cite the original scan/PDF.
NARA API behavior changes over time; if direct API calls fail, use `search_official_declass_sources`
to generate manual search URLs while adapter behavior is updated.
