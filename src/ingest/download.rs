use crate::ingest::download_persist::{
    build_fetched_download, build_revalidated_download, load_cached_entry, persist_download,
    PendingCachePersist,
};
use crate::ingest::redirect::{send_with_redirects, RedirectFollowError, RedirectPolicy};
use crate::sources::{SourceAsset, SourceAssetRole};
use crate::store::files::FileStoreError;
use crate::store::{
    BlobKind, CacheEntry, CachePolicy, CacheStore, ContentAddressedStore, StoreError,
};
use reqwest::header::{CONTENT_TYPE, ETAG, LAST_MODIFIED};
use reqwest::{redirect::Policy, Client, StatusCode, Url};
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
    pub redirect_policy: RedirectPolicy,
    pub force: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DownloadCacheStatus {
    Fetched,
    Revalidated,
    NotPersisted,
}

impl DownloadCacheStatus {
    pub(crate) fn as_str(&self) -> &'static str {
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
    pub final_url: String,
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
    RedirectDenied {
        url: String,
        location: Option<String>,
        message: String,
    },
    UnsafeRedirect {
        url: String,
        location: String,
        message: String,
    },
    TooManyRedirects {
        url: String,
        max_hops: usize,
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
            Self::RedirectDenied {
                url,
                location,
                message,
            } => {
                write!(f, "redirect denied for {url}: {message}")?;
                if let Some(location) = location {
                    write!(f, " Location: {location}")?;
                }
                Ok(())
            }
            Self::UnsafeRedirect {
                url,
                location,
                message,
            } => write!(
                f,
                "unsafe redirect rejected for {url}: {message}. Location: {location}"
            ),
            Self::TooManyRedirects { url, max_hops } => write!(
                f,
                "asset download exceeded redirect hop limit for {url}: max {max_hops}"
            ),
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

impl From<RedirectFollowError> for DownloadError {
    fn from(err: RedirectFollowError) -> Self {
        match err {
            RedirectFollowError::Request(err) => Self::Request(err),
            RedirectFollowError::Denied {
                url,
                location,
                message,
            } => Self::RedirectDenied {
                url,
                location,
                message,
            },
            RedirectFollowError::Unsafe {
                url,
                location,
                message,
            } => Self::UnsafeRedirect {
                url,
                location,
                message,
            },
            RedirectFollowError::TooMany { url, max_hops } => {
                Self::TooManyRedirects { url, max_hops }
            }
        }
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
        let cached = self.load_cached_entry(cache, &request)?;
        let prepared = self.download_http(files, request, cached).await?;
        self.persist_prepared_download(cache, prepared)
    }

    pub(crate) fn load_cached_entry(
        &self,
        cache: &CacheStore<'_>,
        request: &AssetDownloadRequest<'_>,
    ) -> DownloadResult<Option<CacheEntry>> {
        let key = cache_key(request.source, &request.asset.asset_url);
        load_cached_entry(cache, request, &key)
    }

    pub(crate) async fn download_http(
        &self,
        files: &ContentAddressedStore,
        request: AssetDownloadRequest<'_>,
        cached: Option<CacheEntry>,
    ) -> DownloadResult<PendingCachePersist> {
        let initial_url = validate_asset_url(&request.asset.asset_url)?;
        let cache_key = cache_key(request.source, &request.asset.asset_url);
        let (response, final_url) = send_with_redirects(
            &self.client,
            &initial_url,
            cached.as_ref(),
            request.redirect_policy,
            USER_AGENT_VALUE,
        )
        .await
        .map_err(DownloadError::from)?;
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
            return build_revalidated_download(
                &request,
                cached,
                cache_key,
                response_headers_json,
                etag,
                last_modified,
            );
        }
        if !status.is_success() {
            return Err(DownloadError::HttpStatus {
                url: final_url.to_string(),
                status,
            });
        }
        if let Some(length) = response.content_length() {
            if length > self.max_bytes {
                return Err(DownloadError::TooLarge {
                    url: final_url.to_string(),
                    limit: self.max_bytes,
                    actual: length,
                });
            }
        }

        let body = self.read_bounded(response, final_url.as_str()).await?;
        let stored =
            files.put_reader(blob_kind(&request.asset.role), Cursor::new(body.as_slice()))?;
        build_fetched_download(
            &request,
            cache_key,
            final_url.as_str(),
            status,
            response_headers_json,
            etag,
            last_modified,
            mime_type,
            &headers,
            stored,
        )
    }

    pub(crate) fn persist_prepared_download(
        &self,
        cache: &CacheStore<'_>,
        prepared: PendingCachePersist,
    ) -> DownloadResult<DownloadedAsset> {
        persist_download(cache, prepared)
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

fn validate_asset_url(url: &str) -> DownloadResult<Url> {
    let parsed = Url::parse(url).map_err(|err| DownloadError::InvalidUrl {
        url: url.to_owned(),
        message: err.to_string(),
    })?;
    if matches!(parsed.scheme(), "http" | "https") {
        Ok(parsed)
    } else {
        Err(DownloadError::InvalidUrl {
            url: url.to_owned(),
            message: "only http and https asset URLs are supported".to_owned(),
        })
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
            reqwest::header::CACHE_CONTROL,
            "max-age=60, no-store".parse().expect("header"),
        );
        assert_eq!(
            crate::ingest::download_persist::effective_cache_policy(
                CachePolicy::RespectSourceHeaders,
                &headers,
            ),
            CachePolicy::DoNotPersist
        );
    }
}
