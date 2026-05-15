use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::json;
use std::fmt;
use std::path::Path;

const MIGRATIONS: &[(&str, &str)] = &[("001_initial", include_str!("migrations/001_initial.sql"))];

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    InvalidDocumentKey(String),
    InvalidJson {
        field: &'static str,
        value: String,
    },
    InvalidPageRange(String),
    MissingDocument(String),
    MissingIngestionJob(String),
    MissingPages {
        document_id: String,
        page_start: u32,
        page_end: u32,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "sqlite error: {err}"),
            Self::InvalidDocumentKey(key) => write!(f, "invalid document_key: {key}"),
            Self::InvalidJson { field, value } => write!(f, "invalid JSON for {field}: {value}"),
            Self::InvalidPageRange(message) => write!(f, "invalid page range: {message}"),
            Self::MissingDocument(key) => write!(f, "document not found: {key}"),
            Self::MissingIngestionJob(key) => write!(f, "ingestion job not found: {key}"),
            Self::MissingPages {
                document_id,
                page_start,
                page_end,
            } => write!(
                f,
                "pages {page_start}-{page_end} not found for document: {document_id}"
            ),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DocumentKey(String);

impl DocumentKey {
    pub fn new(value: impl Into<String>) -> StoreResult<Self> {
        let value = value.into();
        if is_document_key_safe(&value) {
            Ok(Self(value))
        } else {
            Err(StoreError::InvalidDocumentKey(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DocumentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_document_key_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetRole {
    Pdf,
    Html,
    OcrText,
    Transcript,
    Image,
    Other,
}

impl AssetRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Html => "html",
            Self::OcrText => "ocr_text",
            Self::Transcript => "transcript",
            Self::Image => "image",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextSource {
    EmbeddedPdfText,
    SourceOcr,
    LocalOcr,
    Html,
    Tei,
    ApiText,
}

impl TextSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::EmbeddedPdfText => "embedded_pdf_text",
            Self::SourceOcr => "source_ocr",
            Self::LocalOcr => "local_ocr",
            Self::Html => "html",
            Self::Tei => "tei",
            Self::ApiText => "api_text",
        }
    }
}

#[derive(Clone, Debug)]
pub struct UpsertDocument {
    pub public_id: String,
    pub document_key: DocumentKey,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub date: Option<String>,
    pub collection: Option<String>,
    pub record_group: Option<String>,
    pub description: Option<String>,
    pub origin_url: Option<String>,
    pub document_url: Option<String>,
    pub pdf_url: Option<String>,
    pub metadata_json: String,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StoredDocument {
    pub id: i64,
    pub public_id: String,
    pub document_key: DocumentKey,
    pub source: String,
    pub source_id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredDocumentMetadata {
    pub id: i64,
    pub public_id: String,
    pub document_key: DocumentKey,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub date: Option<String>,
    pub collection: Option<String>,
    pub record_group: Option<String>,
    pub description: Option<String>,
    pub origin_url: Option<String>,
    pub document_url: Option<String>,
    pub pdf_url: Option<String>,
    pub metadata_json: String,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
    pub page_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredPageText {
    pub page_number: u32,
    pub text: String,
    pub text_source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewIngestionJob {
    pub job_key: String,
    pub operation: String,
    pub source: String,
    pub source_id: Option<String>,
    pub target_url: Option<String>,
    pub next_action: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredIngestionJob {
    pub job_key: String,
    pub source: String,
    pub source_id: Option<String>,
    pub target_url: Option<String>,
    pub status: String,
    pub stage: String,
    pub progress: f64,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub next_action: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AssetInput {
    pub document_key: DocumentKey,
    pub asset_url: String,
    pub mime_type: Option<String>,
    pub role: AssetRole,
    pub sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub fetched_at: Option<String>,
    pub cache_policy: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PageInput {
    pub document_key: DocumentKey,
    pub page_number: i64,
    pub text: String,
    pub text_source: TextSource,
    pub quality_score: Option<f64>,
    pub warnings_json: String,
}

#[derive(Clone, Debug)]
pub struct ChunkInput {
    pub document_key: DocumentKey,
    pub chunk_id: String,
    pub page_start: i64,
    pub page_end: i64,
    pub text: String,
    pub token_estimate: Option<i64>,
    pub metadata_json: String,
}

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    pub fn open_memory() -> StoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn)
    }

    pub fn from_connection(conn: Connection) -> StoreResult<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5_000)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn migrate(&self) -> StoreResult<()> {
        for (_version, sql) in MIGRATIONS {
            self.conn.execute_batch(sql)?;
        }
        Ok(())
    }

    pub fn upsert_document(&self, document: &UpsertDocument) -> StoreResult<StoredDocument> {
        ensure_json_object("metadata_json", &document.metadata_json)?;
        if document.public_id == document.document_key.as_str()
            || document.source_id == document.document_key.as_str()
        {
            return Err(StoreError::InvalidDocumentKey(
                document.document_key.to_string(),
            ));
        }

        self.conn.execute(
            "
            INSERT INTO documents (
                public_id, document_key, source, source_id, title, date, collection,
                record_group, description, origin_url, document_url, pdf_url,
                metadata_json, citation_note, terms_note
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(document_key) DO UPDATE SET
                public_id = excluded.public_id,
                source = excluded.source,
                source_id = excluded.source_id,
                title = excluded.title,
                date = excluded.date,
                collection = excluded.collection,
                record_group = excluded.record_group,
                description = excluded.description,
                origin_url = excluded.origin_url,
                document_url = excluded.document_url,
                pdf_url = excluded.pdf_url,
                metadata_json = excluded.metadata_json,
                citation_note = excluded.citation_note,
                terms_note = excluded.terms_note,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            ",
            params![
                document.public_id,
                document.document_key.as_str(),
                document.source,
                document.source_id,
                document.title,
                document.date,
                document.collection,
                document.record_group,
                document.description,
                document.origin_url,
                document.document_url,
                document.pdf_url,
                document.metadata_json,
                document.citation_note,
                document.terms_note,
            ],
        )?;
        self.get_document_by_key(&document.document_key)
    }

    pub fn get_document_by_key(&self, document_key: &DocumentKey) -> StoreResult<StoredDocument> {
        self.conn
            .query_row(
                "
                SELECT id, public_id, document_key, source, source_id, title
                FROM documents
                WHERE document_key = ?1
                ",
                [document_key.as_str()],
                read_document,
            )
            .optional()?
            .ok_or_else(|| StoreError::MissingDocument(document_key.to_string()))
    }

    pub fn get_document_metadata(
        &self,
        public_id_or_document_key: &str,
    ) -> StoreResult<StoredDocumentMetadata> {
        self.conn
            .query_row(
                "
                SELECT d.id, d.public_id, d.document_key, d.source, d.source_id,
                    d.title, d.date, d.collection, d.record_group, d.description,
                    d.origin_url, d.document_url, d.pdf_url, d.metadata_json,
                    d.citation_note, d.terms_note, COUNT(p.id) AS page_count
                FROM documents d
                LEFT JOIN pages p ON p.document_id = d.id
                WHERE d.public_id = ?1 OR d.document_key = ?1
                GROUP BY d.id
                ",
                [public_id_or_document_key],
                read_document_metadata,
            )
            .optional()?
            .ok_or_else(|| StoreError::MissingDocument(public_id_or_document_key.to_owned()))
    }

    pub fn get_page_text(
        &self,
        public_id_or_document_key: &str,
        page_start: u32,
        page_end: u32,
    ) -> StoreResult<Vec<StoredPageText>> {
        if page_start == 0 || page_end == 0 {
            return Err(StoreError::InvalidPageRange(
                "page_start and page_end must be one-based".to_owned(),
            ));
        }
        if page_start > page_end {
            return Err(StoreError::InvalidPageRange(
                "page_start must be less than or equal to page_end".to_owned(),
            ));
        }

        let document = self.get_document_metadata(public_id_or_document_key)?;
        let mut stmt = self.conn.prepare(
            "
            SELECT page_number, text, text_source
            FROM pages
            WHERE document_id = ?1 AND page_number BETWEEN ?2 AND ?3
            ORDER BY page_number
            ",
        )?;
        let rows = stmt.query_map(
            params![document.id, i64::from(page_start), i64::from(page_end)],
            read_page_text,
        )?;
        let pages = collect_pages(rows)?;
        let expected_count = page_end - page_start + 1;
        if pages.len() != expected_count as usize {
            return Err(StoreError::MissingPages {
                document_id: public_id_or_document_key.to_owned(),
                page_start,
                page_end,
            });
        }
        Ok(pages)
    }

    pub fn add_asset(&self, asset: &AssetInput) -> StoreResult<i64> {
        let document = self.get_document_by_key(&asset.document_key)?;
        self.conn.execute(
            "
            INSERT INTO assets (
                document_id, asset_url, mime_type, role, sha256, size_bytes, etag,
                last_modified, fetched_at, cache_policy
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(document_id, asset_url, role) DO UPDATE SET
                mime_type = excluded.mime_type,
                sha256 = excluded.sha256,
                size_bytes = excluded.size_bytes,
                etag = excluded.etag,
                last_modified = excluded.last_modified,
                fetched_at = excluded.fetched_at,
                cache_policy = excluded.cache_policy
            ",
            params![
                document.id,
                asset.asset_url,
                asset.mime_type,
                asset.role.as_str(),
                asset.sha256,
                asset.size_bytes,
                asset.etag,
                asset.last_modified,
                asset.fetched_at,
                asset.cache_policy,
            ],
        )?;
        self.conn
            .query_row(
                "
                SELECT id
                FROM assets
                WHERE document_id = ?1 AND asset_url = ?2 AND role = ?3
                ",
                params![document.id, asset.asset_url, asset.role.as_str()],
                |row| row.get(0),
            )
            .map_err(StoreError::from)
    }

    pub fn create_ingestion_job(
        &mut self,
        job: &NewIngestionJob,
    ) -> StoreResult<StoredIngestionJob> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "
            INSERT OR IGNORE INTO ingestion_jobs (
                job_key, source, source_id, target_url, status, stage, progress, warnings,
                next_action
            )
            VALUES (?1, ?2, ?3, ?4, 'queued', 'queued', 0.0, '[]', ?5)
            ",
            params![
                job.job_key,
                job.source,
                job.source_id,
                job.target_url,
                job.next_action,
            ],
        )?;

        if tx.changes() == 1 {
            let payload_json = json!({
                "job_key": job.job_key,
                "operation": job.operation,
                "source": job.source,
                "source_id": job.source_id,
                "target_url": job.target_url,
            })
            .to_string();
            tx.execute(
                "
                INSERT INTO outbox (topic, payload_json)
                VALUES ('ingestion.job.queued', ?1)
                ",
                [payload_json],
            )?;
        }

        let stored = get_ingestion_job_by_key_tx(&tx, &job.job_key)?;
        tx.commit()?;
        Ok(stored)
    }

    pub fn get_ingestion_job_by_key(&self, job_key: &str) -> StoreResult<StoredIngestionJob> {
        self.conn
            .query_row(
                "
                SELECT job_key, source, source_id, target_url, status, stage, progress, error,
                       warnings, next_action
                FROM ingestion_jobs
                WHERE job_key = ?1
                ",
                [job_key],
                read_ingestion_job,
            )
            .optional()?
            .ok_or_else(|| StoreError::MissingIngestionJob(job_key.to_owned()))
    }

    pub fn replace_pages_and_chunks(
        &mut self,
        document_key: &DocumentKey,
        pages: &[PageInput],
        chunks: &[ChunkInput],
    ) -> StoreResult<()> {
        let tx = self.conn.transaction()?;
        let document = get_document_by_key_tx(&tx, document_key)?;
        tx.execute("DELETE FROM pages WHERE document_id = ?1", [document.id])?;
        tx.execute("DELETE FROM chunks WHERE document_id = ?1", [document.id])?;
        tx.execute(
            "DELETE FROM chunk_fts WHERE document_key = ?1",
            [document_key.as_str()],
        )?;

        for page in pages {
            ensure_json_array("warnings_json", &page.warnings_json)?;
            tx.execute(
                "
                INSERT INTO pages (
                    document_id, page_number, text, text_source, quality_score, warnings
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ",
                params![
                    document.id,
                    page.page_number,
                    page.text,
                    page.text_source.as_str(),
                    page.quality_score,
                    page.warnings_json,
                ],
            )?;
        }

        for chunk in chunks {
            ensure_json_object("metadata_json", &chunk.metadata_json)?;
            tx.execute(
                "
                INSERT INTO chunks (
                    document_id, chunk_id, page_start, page_end, text, token_estimate, metadata_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ",
                params![
                    document.id,
                    chunk.chunk_id,
                    chunk.page_start,
                    chunk.page_end,
                    chunk.text,
                    chunk.token_estimate,
                    chunk.metadata_json,
                ],
            )?;
            tx.execute(
                "
                INSERT INTO chunk_fts (
                    document_key, chunk_id, source, title, body, page_start, page_end
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ",
                params![
                    document.document_key.as_str(),
                    chunk.chunk_id,
                    document.source,
                    document.title,
                    chunk.text,
                    chunk.page_start,
                    chunk.page_end,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }
}

fn read_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredDocument> {
    let key: String = row.get(2)?;
    Ok(StoredDocument {
        id: row.get(0)?,
        public_id: row.get(1)?,
        document_key: DocumentKey(key),
        source: row.get(3)?,
        source_id: row.get(4)?,
        title: row.get(5)?,
    })
}

fn read_document_metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredDocumentMetadata> {
    let key: String = row.get(2)?;
    let page_count: i64 = row.get(16)?;
    Ok(StoredDocumentMetadata {
        id: row.get(0)?,
        public_id: row.get(1)?,
        document_key: DocumentKey(key),
        source: row.get(3)?,
        source_id: row.get(4)?,
        title: row.get(5)?,
        date: row.get(6)?,
        collection: row.get(7)?,
        record_group: row.get(8)?,
        description: row.get(9)?,
        origin_url: row.get(10)?,
        document_url: row.get(11)?,
        pdf_url: row.get(12)?,
        metadata_json: row.get(13)?,
        citation_note: row.get(14)?,
        terms_note: row.get(15)?,
        page_count: page_count.max(0) as u32,
    })
}

fn read_page_text(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredPageText> {
    let page_number: i64 = row.get(0)?;
    Ok(StoredPageText {
        page_number: page_number.max(0) as u32,
        text: row.get(1)?,
        text_source: row.get(2)?,
    })
}

fn collect_pages<F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<StoredPageText>, StoreError>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<StoredPageText>,
{
    let mut pages = Vec::new();
    for page in rows {
        pages.push(page?);
    }
    Ok(pages)
}

fn get_ingestion_job_by_key_tx(
    tx: &Transaction<'_>,
    job_key: &str,
) -> StoreResult<StoredIngestionJob> {
    tx.query_row(
        "
        SELECT job_key, source, source_id, target_url, status, stage, progress, error, warnings,
               next_action
        FROM ingestion_jobs
        WHERE job_key = ?1
        ",
        [job_key],
        read_ingestion_job,
    )
    .optional()?
    .ok_or_else(|| StoreError::MissingIngestionJob(job_key.to_owned()))
}

fn read_ingestion_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredIngestionJob> {
    let warnings_json: String = row.get(8)?;
    let warnings = serde_json::from_str::<Vec<String>>(&warnings_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(err))
    })?;

    Ok(StoredIngestionJob {
        job_key: row.get(0)?,
        source: row.get(1)?,
        source_id: row.get(2)?,
        target_url: row.get(3)?,
        status: row.get(4)?,
        stage: row.get(5)?,
        progress: row.get(6)?,
        error: row.get(7)?,
        warnings,
        next_action: row.get(9)?,
    })
}

fn get_document_by_key_tx(
    tx: &Transaction<'_>,
    document_key: &DocumentKey,
) -> StoreResult<StoredDocument> {
    tx.query_row(
        "
        SELECT id, public_id, document_key, source, source_id, title
        FROM documents
        WHERE document_key = ?1
        ",
        [document_key.as_str()],
        read_document,
    )
    .optional()?
    .ok_or_else(|| StoreError::MissingDocument(document_key.to_string()))
}

fn ensure_json_object(field: &'static str, value: &str) -> StoreResult<()> {
    if value.trim_start().starts_with('{') && value.trim_end().ends_with('}') {
        Ok(())
    } else {
        Err(StoreError::InvalidJson {
            field,
            value: value.to_owned(),
        })
    }
}

fn ensure_json_array(field: &'static str, value: &str) -> StoreResult<()> {
    if value.trim_start().starts_with('[') && value.trim_end().ends_with(']') {
        Ok(())
    } else {
        Err(StoreError::InvalidJson {
            field,
            value: value.to_owned(),
        })
    }
}
