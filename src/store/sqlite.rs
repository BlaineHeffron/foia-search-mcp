use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::fmt;
use std::path::Path;

const MIGRATIONS: &[(&str, &str)] = &[("001_initial", include_str!("migrations/001_initial.sql"))];

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    InvalidDocumentKey(String),
    InvalidJson { field: &'static str, value: String },
    MissingDocument(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "sqlite error: {err}"),
            Self::InvalidDocumentKey(key) => write!(f, "invalid document_key: {key}"),
            Self::InvalidJson { field, value } => write!(f, "invalid JSON for {field}: {value}"),
            Self::MissingDocument(key) => write!(f, "document not found: {key}"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_is_idempotent() {
        let store = SqliteStore::open_memory().expect("open in-memory store");
        store.migrate().expect("second migration run");

        let table_count: i64 = store
            .connection()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type IN ('table', 'index')",
                [],
                |row| row.get(0),
            )
            .expect("count sqlite objects");
        assert!(table_count > 0);
    }

    #[test]
    fn document_key_must_not_be_source_id_or_public_id() {
        let store = SqliteStore::open_memory().expect("open in-memory store");
        let document = UpsertDocument {
            public_id: "cia:CREST/unsafe/id".to_owned(),
            document_key: DocumentKey::new("CREST_unsafe_id").expect("safe key"),
            source: "cia".to_owned(),
            source_id: "CREST/unsafe/id".to_owned(),
            title: "Test".to_owned(),
            date: None,
            collection: None,
            record_group: None,
            description: None,
            origin_url: None,
            document_url: None,
            pdf_url: None,
            metadata_json: "{}".to_owned(),
            citation_note: None,
            terms_note: None,
        };

        let stored = store.upsert_document(&document).expect("insert document");
        assert_eq!(stored.document_key.as_str(), "CREST_unsafe_id");
        assert_ne!(stored.document_key.as_str(), stored.source_id);
    }

    #[test]
    fn unsafe_document_keys_are_rejected() {
        assert!(DocumentKey::new("CREST/unsafe/id").is_err());
        assert!(DocumentKey::new("../escape").is_err());
        assert!(DocumentKey::new("").is_err());
    }

    #[test]
    fn asset_upsert_returns_stable_row_id() {
        let store = SqliteStore::open_memory().expect("open in-memory store");
        let key = DocumentKey::new("doc_cia_asset").expect("safe key");
        store
            .upsert_document(&UpsertDocument {
                public_id: "cia:asset-test".to_owned(),
                document_key: key.clone(),
                source: "cia".to_owned(),
                source_id: "asset-test".to_owned(),
                title: "Asset Test".to_owned(),
                date: None,
                collection: None,
                record_group: None,
                description: None,
                origin_url: None,
                document_url: None,
                pdf_url: None,
                metadata_json: "{}".to_owned(),
                citation_note: None,
                terms_note: None,
            })
            .expect("insert document");

        let first = store
            .add_asset(&AssetInput {
                document_key: key.clone(),
                asset_url: "https://www.cia.gov/readingroom/docs/test.pdf".to_owned(),
                mime_type: Some("application/pdf".to_owned()),
                role: AssetRole::Pdf,
                sha256: Some("a".repeat(64)),
                size_bytes: Some(10),
                etag: None,
                last_modified: None,
                fetched_at: None,
                cache_policy: Some("respect_source_headers".to_owned()),
            })
            .expect("insert asset");
        let second = store
            .add_asset(&AssetInput {
                document_key: key,
                asset_url: "https://www.cia.gov/readingroom/docs/test.pdf".to_owned(),
                mime_type: Some("application/pdf".to_owned()),
                role: AssetRole::Pdf,
                sha256: Some("b".repeat(64)),
                size_bytes: Some(20),
                etag: None,
                last_modified: None,
                fetched_at: None,
                cache_policy: Some("respect_source_headers".to_owned()),
            })
            .expect("update asset");

        assert_eq!(first, second);
    }
}
