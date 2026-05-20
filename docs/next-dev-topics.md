# Next Development Topics

Status as of `5ae2d0b docs(index): add phase 9 evaluation plan`: the
worktree was clean and `main` was pushed.

## Pick Up Here

1. Decide whether to pause source expansion and start the Phase 9 indexing
   evaluation described in `docs/phase9-indexing-evaluation.md`. Tantivy is the
   likely first candidate; LanceDB should wait for a defined embedding model and
   a measurable eval improvement.
2. Decide whether current adapters need more fixture/live edge-case hardening
   before indexing work begins.
3. Resolve policy questions that affect future interfaces:
   - Which OCR modes should eventually be exposed.
   - Whether trusted direct-ingestion allowlists should ship as examples.
   - What cache policy is acceptable for NARA-derived metadata.
   - How deep repair-surface eval coverage should go.
4. Keep repair surfaces explicit and operator-confirmed. Do not add startup,
   runtime, or worker auto-repair wiring.
5. If source expansion resumes, choose the next official source by research
   demand and source stability, and require fixtures, source warning/citation
   notes, cache/redirect policy contracts, and at least one eval.

## Current Constraints

- Do not grow oversized Rust files such as `src/mcp/tools.rs`; add focused
  submodules or tests instead.
- Preserve source citation, terms, and warning notes through search, ingestion,
  and local retrieval.
- Keep redirects default-deny unless a specific future source requires an
  explicit reviewed policy.
- Keep `tesseract` unavailable unless a concrete OCR need justifies a real
  backend slice.
