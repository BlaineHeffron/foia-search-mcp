use crate::ingest::download::{
    AssetDownloadRequest, DownloadCacheStatus, DownloadError, DownloadResult, DownloadedAsset,
};
use crate::store::files::StoredBlob;
use crate::store::{CacheEntry, CachePolicy, CacheStore};
use reqwest::header::CACHE_CONTROL;
use reqwest::{header::HeaderMap, StatusCode};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct PendingCachePersist {
    pub(crate) downloaded: DownloadedAsset,
    pub(crate) cache_entry: Option<CacheEntry>,
}

pub(crate) fn load_cached_entry(
    cache: &CacheStore<'_>,
    request: &AssetDownloadRequest<'_>,
    cache_key: &str,
) -> DownloadResult<Option<CacheEntry>> {
    if request.cache_policy == CachePolicy::RespectSourceHeaders && !request.force {
        Ok(cache.get(cache_key)?)
    } else {
        Ok(None)
    }
}

pub(crate) fn persist_download(
    cache: &CacheStore<'_>,
    pending: PendingCachePersist,
) -> DownloadResult<DownloadedAsset> {
    if let Some(entry) = pending.cache_entry.as_ref() {
        cache.put(entry)?;
    }
    Ok(pending.downloaded)
}

pub(crate) fn build_revalidated_download(
    request: &AssetDownloadRequest<'_>,
    cached: Option<CacheEntry>,
    cache_key: String,
    response_headers_json: String,
    response_etag: Option<String>,
    response_last_modified: Option<String>,
) -> DownloadResult<PendingCachePersist> {
    let entry = cached.ok_or_else(|| DownloadError::MissingCachedBody {
        cache_key: cache_key.clone(),
    })?;
    let sha256 = entry
        .body_sha256
        .clone()
        .ok_or_else(|| DownloadError::MissingCachedBody {
            cache_key: cache_key.clone(),
        })?;
    let path = entry
        .body_path
        .clone()
        .ok_or_else(|| DownloadError::MissingCachedBody {
            cache_key: cache_key.clone(),
        })?;
    let size_bytes = std::fs::metadata(&path)
        .map(|metadata| metadata.len())
        .map_err(|_| DownloadError::MissingCachedBody {
            cache_key: cache_key.clone(),
        })?;
    let etag = response_etag.or(entry.etag);
    let last_modified = response_last_modified.or(entry.last_modified);
    let provenance_json = provenance_json(&Provenance {
        cache_key: &cache_key,
        source: request.source,
        asset_url: &request.asset.asset_url,
        final_url: &request.asset.asset_url,
        method: "GET",
        status_code: StatusCode::NOT_MODIFIED.as_u16(),
        cache_status: DownloadCacheStatus::Revalidated.as_str(),
        cache_policy: cache_policy_str(entry.cache_policy),
        sha256: &sha256,
        size_bytes,
        body_path: Some(&path),
        etag: etag.as_deref(),
        last_modified: last_modified.as_deref(),
        response_headers_json: &response_headers_json,
    })?;

    Ok(PendingCachePersist {
        downloaded: DownloadedAsset {
            cache_key,
            source: request.source.to_owned(),
            asset_url: request.asset.asset_url.clone(),
            final_url: request.asset.asset_url.clone(),
            mime_type: request.asset.mime_type.clone(),
            role: request.asset.role.clone(),
            status_code: StatusCode::NOT_MODIFIED.as_u16(),
            sha256,
            path: PathBuf::from(path),
            size_bytes,
            etag,
            last_modified,
            cache_policy: entry.cache_policy,
            cache_status: DownloadCacheStatus::Revalidated,
            response_headers_json,
            provenance_json,
        },
        cache_entry: None,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_fetched_download(
    request: &AssetDownloadRequest<'_>,
    cache_key: String,
    final_url: &str,
    status: StatusCode,
    response_headers_json: String,
    etag: Option<String>,
    last_modified: Option<String>,
    mime_type: Option<String>,
    headers: &HeaderMap,
    stored: StoredBlob,
) -> DownloadResult<PendingCachePersist> {
    let effective_policy = effective_cache_policy(request.cache_policy, headers);
    let cache_status = if effective_policy == CachePolicy::DoNotPersist {
        DownloadCacheStatus::NotPersisted
    } else {
        DownloadCacheStatus::Fetched
    };
    let provenance_json = provenance_json(&Provenance {
        cache_key: &cache_key,
        source: request.source,
        asset_url: &request.asset.asset_url,
        final_url,
        method: "GET",
        status_code: status.as_u16(),
        cache_status: cache_status.as_str(),
        cache_policy: cache_policy_str(effective_policy),
        sha256: &stored.sha256,
        size_bytes: stored.size_bytes,
        body_path: Some(stored.path.to_string_lossy().as_ref()),
        etag: etag.as_deref(),
        last_modified: last_modified.as_deref(),
        response_headers_json: &response_headers_json,
    })?;

    let cache_entry = CacheEntry {
        cache_key: cache_key.clone(),
        source: request.source.to_owned(),
        url: request.asset.asset_url.clone(),
        method: "GET".to_owned(),
        status_code: Some(i64::from(status.as_u16())),
        response_headers_json: response_headers_json.clone(),
        body_sha256: Some(stored.sha256.clone()),
        body_path: Some(stored.path.to_string_lossy().into_owned()),
        etag: etag.clone(),
        last_modified: last_modified.clone(),
        expires_at: None,
        cache_policy: effective_policy,
        provenance_json: provenance_json.clone(),
    };

    Ok(PendingCachePersist {
        downloaded: DownloadedAsset {
            cache_key,
            source: request.source.to_owned(),
            asset_url: request.asset.asset_url.clone(),
            final_url: final_url.to_owned(),
            mime_type,
            role: request.asset.role.clone(),
            status_code: status.as_u16(),
            sha256: stored.sha256,
            path: stored.path,
            size_bytes: stored.size_bytes,
            etag,
            last_modified,
            cache_policy: effective_policy,
            cache_status,
            response_headers_json,
            provenance_json,
        },
        cache_entry: Some(cache_entry),
    })
}

pub(crate) fn effective_cache_policy(policy: CachePolicy, headers: &HeaderMap) -> CachePolicy {
    if policy == CachePolicy::DoNotPersist {
        return CachePolicy::DoNotPersist;
    }
    let cache_control = headers
        .get(CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if cache_control
        .split(',')
        .map(str::trim)
        .any(|directive| matches!(directive, "no-store" | "private"))
    {
        CachePolicy::DoNotPersist
    } else {
        CachePolicy::RespectSourceHeaders
    }
}

fn provenance_json(provenance: &Provenance<'_>) -> Result<String, serde_json::Error> {
    serde_json::to_string(provenance)
}

fn cache_policy_str(policy: CachePolicy) -> &'static str {
    match policy {
        CachePolicy::RespectSourceHeaders => "respect_source_headers",
        CachePolicy::DoNotPersist => "do_not_persist",
    }
}

#[derive(Serialize)]
struct Provenance<'a> {
    cache_key: &'a str,
    source: &'a str,
    asset_url: &'a str,
    final_url: &'a str,
    method: &'a str,
    status_code: u16,
    cache_status: &'a str,
    cache_policy: &'a str,
    sha256: &'a str,
    size_bytes: u64,
    body_path: Option<&'a str>,
    etag: Option<&'a str>,
    last_modified: Option<&'a str>,
    response_headers_json: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SqliteStore;

    #[test]
    fn respect_source_headers_no_store_becomes_do_not_persist() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CACHE_CONTROL,
            "max-age=60, private".parse().expect("header"),
        );
        assert_eq!(
            effective_cache_policy(CachePolicy::RespectSourceHeaders, &headers),
            CachePolicy::DoNotPersist
        );
    }

    #[test]
    fn persist_download_deletes_cached_row_for_do_not_persist() {
        let store = SqliteStore::open_memory().expect("open store");
        let cache = CacheStore::new(&store);
        let key = cache_key("cia", "https://example.test/asset.pdf");
        let existing = CacheEntry {
            cache_key: key.clone(),
            source: "cia".to_owned(),
            url: "https://example.test/asset.pdf".to_owned(),
            method: "GET".to_owned(),
            status_code: Some(200),
            response_headers_json: "{}".to_owned(),
            body_sha256: Some("a".repeat(64)),
            body_path: Some("/tmp/fake.pdf".to_owned()),
            etag: None,
            last_modified: None,
            expires_at: None,
            cache_policy: CachePolicy::RespectSourceHeaders,
            provenance_json: "{}".to_owned(),
        };
        cache.put(&existing).expect("seed cache row");

        let persisted = PendingCachePersist {
            downloaded: DownloadedAsset {
                cache_key: key.clone(),
                source: "cia".to_owned(),
                asset_url: "https://example.test/asset.pdf".to_owned(),
                final_url: "https://example.test/asset.pdf".to_owned(),
                mime_type: Some("application/pdf".to_owned()),
                role: crate::sources::SourceAssetRole::Pdf,
                status_code: 200,
                sha256: "a".repeat(64),
                path: PathBuf::from("/tmp/fake.pdf"),
                size_bytes: 3,
                etag: None,
                last_modified: None,
                cache_policy: CachePolicy::DoNotPersist,
                cache_status: DownloadCacheStatus::NotPersisted,
                response_headers_json: "{}".to_owned(),
                provenance_json: "{}".to_owned(),
            },
            cache_entry: Some(CacheEntry {
                cache_key: key.clone(),
                source: "cia".to_owned(),
                url: "https://example.test/asset.pdf".to_owned(),
                method: "GET".to_owned(),
                status_code: Some(200),
                response_headers_json: "{}".to_owned(),
                body_sha256: Some("a".repeat(64)),
                body_path: Some("/tmp/fake.pdf".to_owned()),
                etag: None,
                last_modified: None,
                expires_at: None,
                cache_policy: CachePolicy::DoNotPersist,
                provenance_json: "{}".to_owned(),
            }),
        };
        let _downloaded = persist_download(&cache, persisted).expect("persist");
        assert!(cache.get(&key).expect("read cache").is_none());
    }

    fn cache_key(source: &str, url: &str) -> String {
        crate::ingest::download::cache_key(source, url)
    }
}
