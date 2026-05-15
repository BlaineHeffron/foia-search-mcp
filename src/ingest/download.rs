use crate::sources::{SourceAsset, SourceAssetRole};
use crate::store::files::FileStoreError;
use crate::store::{
    BlobKind, CacheEntry, CachePolicy, CacheStore, ContentAddressedStore, StoreError,
};
use reqwest::header::{
    CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, USER_AGENT,
};
use reqwest::{redirect::Policy, Client, StatusCode, Url};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024;
const USER_AGENT_VALUE: &str = "foia-search-mcp/0.1 (+https://github.com/modelcontextprotocol)";

#[derive(Clone, Debug)]
pub struct AssetDownloader {
    client: Client,
    max_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct AssetDownloadRequest<'a> {
    pub source: &'a str,
    pub asset: &'a SourceAsset,
    pub cache_policy: CachePolicy,
    pub force: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DownloadCacheStatus {
    Fetched,
    Revalidated,
    NotPersisted,
}

impl DownloadCacheStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Fetched => "fetched",
            Self::Revalidated => "revalidated",
            Self::NotPersisted => "not_persisted",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadedAsset {
    pub cache_key: String,
    pub source: String,
    pub asset_url: String,
    pub mime_type: Option<String>,
    pub role: SourceAssetRole,
    pub status_code: u16,
    pub sha256: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub cache_policy: CachePolicy,
    pub cache_status: DownloadCacheStatus,
    pub response_headers_json: String,
    pub provenance_json: String,
}

#[derive(Debug)]
pub enum DownloadError {
    InvalidUrl {
        url: String,
        message: String,
    },
    HttpStatus {
        url: String,
        status: StatusCode,
    },
    TooLarge {
        url: String,
        limit: u64,
        actual: u64,
    },
    MissingCachedBody {
        cache_key: String,
    },
    Request(reqwest::Error),
    FileStore(FileStoreError),
    Cache(StoreError),
    Serialize(serde_json::Error),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl { url, message } => {
                write!(f, "invalid asset URL {url}: {message}")
            }
            Self::HttpStatus { url, status } => {
                write!(f, "asset download failed for {url}: HTTP {status}")
            }
            Self::TooLarge { url, limit, actual } => write!(
                f,
                "asset download exceeded limit for {url}: {actual} bytes > {limit} bytes"
            ),
            Self::MissingCachedBody { cache_key } => {
                write!(f, "cache entry {cache_key} has no retained body")
            }
            Self::Request(err) => write!(f, "asset HTTP request failed: {err}"),
            Self::FileStore(err) => write!(f, "{err}"),
            Self::Cache(err) => write!(f, "{err}"),
            Self::Serialize(err) => write!(f, "failed to serialize download provenance: {err}"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err)
    }
}

impl From<FileStoreError> for DownloadError {
    fn from(err: FileStoreError) -> Self {
        Self::FileStore(err)
    }
}

impl From<StoreError> for DownloadError {
    fn from(err: StoreError) -> Self {
        Self::Cache(err)
    }
}

impl From<serde_json::Error> for DownloadError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialize(err)
    }
}

pub type DownloadResult<T> = Result<T, DownloadError>;

impl AssetDownloader {
    pub fn new() -> DownloadResult<Self> {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .redirect(Policy::none())
            .build()?;
        Ok(Self {
            client,
            max_bytes: DEFAULT_MAX_BYTES,
        })
    }

    pub fn with_client(client: Client, max_bytes: u64) -> Self {
        Self { client, max_bytes }
    }

    pub async fn download(
        &self,
        files: &ContentAddressedStore,
        cache: &CacheStore<'_>,
        request: AssetDownloadRequest<'_>,
    ) -> DownloadResult<DownloadedAsset> {
        validate_asset_url(&request.asset.asset_url)?;
        let cache_key = cache_key(request.source, &request.asset.asset_url);
        let cached = if request.cache_policy == CachePolicy::RespectSourceHeaders && !request.force
        {
            cache.get(&cache_key)?
        } else {
            None
        };

        let mut builder = self
            .client
            .get(&request.asset.asset_url)
            .header(USER_AGENT, USER_AGENT_VALUE);
        if let Some(entry) = cached.as_ref() {
            if let Some(etag) = entry.etag.as_deref() {
                builder = builder.header(IF_NONE_MATCH, etag);
            }
            if let Some(last_modified) = entry.last_modified.as_deref() {
                builder = builder.header(IF_MODIFIED_SINCE, last_modified);
            }
        }

        let response = builder.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let response_headers_json = headers_json(&headers)?;
        let etag = header_string(&headers, ETAG);
        let last_modified = header_string(&headers, LAST_MODIFIED);
        let mime_type = request
            .asset
            .mime_type
            .clone()
            .or_else(|| header_string(&headers, CONTENT_TYPE));

        if status == StatusCode::NOT_MODIFIED {
            return revalidated_asset(
                request,
                cached,
                cache_key,
                response_headers_json,
                etag,
                last_modified,
            );
        }
        if !status.is_success() {
            return Err(DownloadError::HttpStatus {
                url: request.asset.asset_url.clone(),
                status,
            });
        }
        if let Some(length) = response.content_length() {
            if length > self.max_bytes {
                return Err(DownloadError::TooLarge {
                    url: request.asset.asset_url.clone(),
                    limit: self.max_bytes,
                    actual: length,
                });
            }
        }

        let body = self
            .read_bounded(response, &request.asset.asset_url)
            .await?;
        let stored =
            files.put_reader(blob_kind(&request.asset.role), Cursor::new(body.as_slice()))?;
        let effective_policy = effective_cache_policy(request.cache_policy, &headers);
        let cache_status = if effective_policy == CachePolicy::DoNotPersist {
            DownloadCacheStatus::NotPersisted
        } else {
            DownloadCacheStatus::Fetched
        };
        let provenance_json = provenance_json(&Provenance {
            cache_key: &cache_key,
            source: request.source,
            asset_url: &request.asset.asset_url,
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

        cache.put(&CacheEntry {
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
        })?;

        Ok(DownloadedAsset {
            cache_key,
            source: request.source.to_owned(),
            asset_url: request.asset.asset_url.clone(),
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
        })
    }

    async fn read_bounded(
        &self,
        mut response: reqwest::Response,
        url: &str,
    ) -> DownloadResult<Vec<u8>> {
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            let actual = body.len() as u64 + chunk.len() as u64;
            if actual > self.max_bytes {
                return Err(DownloadError::TooLarge {
                    url: url.to_owned(),
                    limit: self.max_bytes,
                    actual,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

pub fn cache_key(source: &str, url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"asset-download:v1\0");
    hasher.update(source.as_bytes());
    hasher.update(b"\0GET\0");
    hasher.update(url.as_bytes());
    format!("asset:sha256:{:x}", hasher.finalize())
}

pub fn store_cache_policy(policy: crate::sources::CachePolicy) -> CachePolicy {
    match policy {
        crate::sources::CachePolicy::RespectSourceHeaders => CachePolicy::RespectSourceHeaders,
        crate::sources::CachePolicy::DoNotPersist => CachePolicy::DoNotPersist,
    }
}

fn revalidated_asset(
    request: AssetDownloadRequest<'_>,
    cached: Option<CacheEntry>,
    cache_key: String,
    response_headers_json: String,
    response_etag: Option<String>,
    response_last_modified: Option<String>,
) -> DownloadResult<DownloadedAsset> {
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

    Ok(DownloadedAsset {
        cache_key,
        source: request.source.to_owned(),
        asset_url: request.asset.asset_url.clone(),
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
    })
}

fn validate_asset_url(url: &str) -> DownloadResult<()> {
    let parsed = Url::parse(url).map_err(|err| DownloadError::InvalidUrl {
        url: url.to_owned(),
        message: err.to_string(),
    })?;
    if matches!(parsed.scheme(), "http" | "https") {
        Ok(())
    } else {
        Err(DownloadError::InvalidUrl {
            url: url.to_owned(),
            message: "only http and https asset URLs are supported".to_owned(),
        })
    }
}

fn effective_cache_policy(
    policy: CachePolicy,
    headers: &reqwest::header::HeaderMap,
) -> CachePolicy {
    if policy == CachePolicy::DoNotPersist {
        return CachePolicy::DoNotPersist;
    }
    let cache_control = header_string(headers, CACHE_CONTROL)
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

fn blob_kind(role: &SourceAssetRole) -> BlobKind {
    match role {
        SourceAssetRole::Pdf => BlobKind::Pdf,
        SourceAssetRole::Html => BlobKind::Html,
        SourceAssetRole::OcrText
        | SourceAssetRole::Transcript
        | SourceAssetRole::Image
        | SourceAssetRole::Other => BlobKind::Other,
    }
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn headers_json(headers: &reqwest::header::HeaderMap) -> Result<String, serde_json::Error> {
    let mut object = serde_json::Map::new();
    for (name, value) in headers {
        let value = value
            .to_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|_| format!("{value:?}"));
        object.insert(name.as_str().to_owned(), Value::String(value));
    }
    serde_json::to_string(&Value::Object(object))
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

    #[test]
    fn cache_key_is_stable_and_source_scoped() {
        let first = cache_key("cia", "https://example.test/doc.pdf");
        assert_eq!(first, cache_key("cia", "https://example.test/doc.pdf"));
        assert_ne!(first, cache_key("nara", "https://example.test/doc.pdf"));
    }

    #[test]
    fn source_cache_policy_maps_to_store_policy() {
        assert_eq!(
            store_cache_policy(crate::sources::CachePolicy::RespectSourceHeaders),
            CachePolicy::RespectSourceHeaders
        );
        assert_eq!(
            store_cache_policy(crate::sources::CachePolicy::DoNotPersist),
            CachePolicy::DoNotPersist
        );
    }

    #[test]
    fn cache_control_no_store_disables_cache_entry() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            CACHE_CONTROL,
            "max-age=60, no-store".parse().expect("header"),
        );
        assert_eq!(
            effective_cache_policy(CachePolicy::RespectSourceHeaders, &headers),
            CachePolicy::DoNotPersist
        );
    }
}
