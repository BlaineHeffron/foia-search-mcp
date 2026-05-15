PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    public_id TEXT NOT NULL UNIQUE,
    document_key TEXT NOT NULL UNIQUE,
    source TEXT NOT NULL,
    source_id TEXT NOT NULL,
    title TEXT NOT NULL,
    date TEXT,
    collection TEXT,
    record_group TEXT,
    description TEXT,
    origin_url TEXT,
    document_url TEXT,
    pdf_url TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    citation_note TEXT,
    terms_note TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (public_id <> document_key),
    CHECK (source_id <> document_key),
    CHECK (length(document_key) > 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_source_id
ON documents(source, source_id);

CREATE INDEX IF NOT EXISTS idx_documents_document_key
ON documents(document_key);

CREATE TABLE IF NOT EXISTS assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    asset_url TEXT NOT NULL,
    mime_type TEXT,
    role TEXT NOT NULL CHECK (role IN (
        'pdf',
        'html',
        'ocr_text',
        'transcript',
        'image',
        'other'
    )),
    sha256 TEXT,
    size_bytes INTEGER,
    etag TEXT,
    last_modified TEXT,
    fetched_at TEXT,
    cache_policy TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(document_id, asset_url, role)
);

CREATE INDEX IF NOT EXISTS idx_assets_document_id
ON assets(document_id);

CREATE INDEX IF NOT EXISTS idx_assets_sha256
ON assets(sha256);

CREATE TABLE IF NOT EXISTS pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    page_number INTEGER NOT NULL CHECK (page_number > 0),
    text TEXT NOT NULL,
    text_source TEXT NOT NULL CHECK (text_source IN (
        'embedded_pdf_text',
        'source_ocr',
        'local_ocr',
        'html',
        'tei',
        'api_text'
    )),
    quality_score REAL,
    warnings TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(document_id, page_number)
);

CREATE INDEX IF NOT EXISTS idx_pages_document_page
ON pages(document_id, page_number);

CREATE TABLE IF NOT EXISTS chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_id TEXT NOT NULL,
    page_start INTEGER NOT NULL CHECK (page_start > 0),
    page_end INTEGER NOT NULL CHECK (page_end >= page_start),
    text TEXT NOT NULL,
    token_estimate INTEGER,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(document_id, chunk_id)
);

CREATE INDEX IF NOT EXISTS idx_chunks_document_pages
ON chunks(document_id, page_start, page_end);

CREATE TABLE IF NOT EXISTS ingestion_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_key TEXT NOT NULL UNIQUE,
    document_id INTEGER REFERENCES documents(id) ON DELETE SET NULL,
    source TEXT NOT NULL,
    source_id TEXT,
    target_url TEXT,
    status TEXT NOT NULL CHECK (status IN (
        'queued',
        'running',
        'succeeded',
        'failed',
        'cancelled',
        'interrupted'
    )),
    stage TEXT NOT NULL,
    progress REAL NOT NULL DEFAULT 0.0 CHECK (progress >= 0.0 AND progress <= 1.0),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_owner TEXT,
    lease_expires_at TEXT,
    error TEXT,
    warnings TEXT NOT NULL DEFAULT '[]',
    next_action TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_ingestion_jobs_status
ON ingestion_jobs(status, updated_at);

CREATE INDEX IF NOT EXISTS idx_ingestion_jobs_document_id
ON ingestion_jobs(document_id);

CREATE TABLE IF NOT EXISTS outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending',
        'running',
        'succeeded',
        'failed'
    )),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error TEXT,
    available_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_outbox_status_available
ON outbox(status, available_at);

CREATE TABLE IF NOT EXISTS cache_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cache_key TEXT NOT NULL UNIQUE,
    source TEXT NOT NULL,
    url TEXT NOT NULL,
    method TEXT NOT NULL DEFAULT 'GET',
    status_code INTEGER,
    response_headers_json TEXT NOT NULL DEFAULT '{}',
    body_sha256 TEXT,
    body_path TEXT,
    etag TEXT,
    last_modified TEXT,
    fetched_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT,
    cache_policy TEXT NOT NULL,
    provenance_json TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_cache_entries_source_url
ON cache_entries(source, url);

CREATE INDEX IF NOT EXISTS idx_cache_entries_expires_at
ON cache_entries(expires_at);

CREATE VIRTUAL TABLE IF NOT EXISTS chunk_fts USING fts5(
    document_key UNINDEXED,
    chunk_id UNINDEXED,
    source UNINDEXED,
    title,
    body,
    page_start UNINDEXED,
    page_end UNINDEXED,
    tokenize = 'unicode61'
);

INSERT OR IGNORE INTO schema_migrations(version) VALUES ('001_initial');
