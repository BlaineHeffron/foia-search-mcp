use crate::store::{SqliteStore, StoreError};
use rusqlite::{params, OptionalExtension};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CachePolicy {
    RespectSourceHeaders,
    DoNotPersist,
}

impl CachePolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::RespectSourceHeaders => "respect_source_headers",
            Self::DoNotPersist => "do_not_persist",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "do_not_persist" => Self::DoNotPersist,
            _ => Self::RespectSourceHeaders,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub cache_key: String,
    pub source: String,
    pub url: String,
    pub method: String,
    pub status_code: Option<i64>,
    pub response_headers_json: String,
    pub body_sha256: Option<String>,
    pub body_path: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub expires_at: Option<String>,
    pub cache_policy: CachePolicy,
    pub provenance_json: String,
}

pub struct CacheStore<'a> {
    store: &'a SqliteStore,
}

impl<'a> CacheStore<'a> {
    pub fn new(store: &'a SqliteStore) -> Self {
        Self { store }
    }

    pub fn put(&self, entry: &CacheEntry) -> Result<(), StoreError> {
        if entry.cache_policy == CachePolicy::DoNotPersist {
            self.store.connection().execute(
                "DELETE FROM cache_entries WHERE cache_key = ?1",
                [entry.cache_key.as_str()],
            )?;
            return Ok(());
        }

        self.store.connection().execute(
            "
            INSERT INTO cache_entries (
                cache_key, source, url, method, status_code, response_headers_json,
                body_sha256, body_path, etag, last_modified, expires_at, cache_policy,
                provenance_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(cache_key) DO UPDATE SET
                source = excluded.source,
                url = excluded.url,
                method = excluded.method,
                status_code = excluded.status_code,
                response_headers_json = excluded.response_headers_json,
                body_sha256 = excluded.body_sha256,
                body_path = excluded.body_path,
                etag = excluded.etag,
                last_modified = excluded.last_modified,
                fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                expires_at = excluded.expires_at,
                cache_policy = excluded.cache_policy,
                provenance_json = excluded.provenance_json
            ",
            params![
                entry.cache_key,
                entry.source,
                entry.url,
                entry.method,
                entry.status_code,
                entry.response_headers_json,
                entry.body_sha256,
                entry.body_path,
                entry.etag,
                entry.last_modified,
                entry.expires_at,
                entry.cache_policy.as_str(),
                entry.provenance_json,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, cache_key: &str) -> Result<Option<CacheEntry>, StoreError> {
        self.store
            .connection()
            .query_row(
                "
                SELECT cache_key, source, url, method, status_code, response_headers_json,
                    body_sha256, body_path, etag, last_modified, expires_at, cache_policy,
                    provenance_json
                FROM cache_entries
                WHERE cache_key = ?1
                  AND cache_policy <> 'do_not_persist'
                  AND (expires_at IS NULL OR expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                ",
                [cache_key],
                |row| {
                    let policy: String = row.get(11)?;
                    Ok(CacheEntry {
                        cache_key: row.get(0)?,
                        source: row.get(1)?,
                        url: row.get(2)?,
                        method: row.get(3)?,
                        status_code: row.get(4)?,
                        response_headers_json: row.get(5)?,
                        body_sha256: row.get(6)?,
                        body_path: row.get(7)?,
                        etag: row.get(8)?,
                        last_modified: row.get(9)?,
                        expires_at: row.get(10)?,
                        cache_policy: CachePolicy::from_str(&policy),
                        provenance_json: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(StoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SqliteStore;

    fn entry(cache_key: &str, policy: CachePolicy, expires_at: Option<&str>) -> CacheEntry {
        CacheEntry {
            cache_key: cache_key.to_owned(),
            source: "nara".to_owned(),
            url: "https://catalog.archives.gov/api/v2/records/search".to_owned(),
            method: "GET".to_owned(),
            status_code: Some(200),
            response_headers_json: "{}".to_owned(),
            body_sha256: None,
            body_path: None,
            etag: None,
            last_modified: None,
            expires_at: expires_at.map(ToOwned::to_owned),
            cache_policy: policy,
            provenance_json: "{}".to_owned(),
        }
    }

    #[test]
    fn do_not_persist_entries_are_not_stored() {
        let store = SqliteStore::open_memory().expect("open in-memory store");
        let cache = CacheStore::new(&store);

        cache
            .put(&entry("nara-key", CachePolicy::DoNotPersist, None))
            .expect("write cache");

        assert!(cache.get("nara-key").expect("read cache").is_none());
    }

    #[test]
    fn expired_entries_are_not_returned() {
        let store = SqliteStore::open_memory().expect("open in-memory store");
        let cache = CacheStore::new(&store);

        cache
            .put(&entry(
                "expired-key",
                CachePolicy::RespectSourceHeaders,
                Some("1970-01-01T00:00:00.000Z"),
            ))
            .expect("write cache");

        assert!(cache.get("expired-key").expect("read cache").is_none());
    }
}
