use crate::ingest::download::DownloadResult;
use crate::ingest::download_persist::PendingCachePersist;
use crate::ingest::source_resolution::{
    resolve_source_record, ResolvedSourceRecord, SourceResolutionError,
};
use crate::ingest::{AssetDownloadRequest, AssetDownloader, RedirectPolicy};
use crate::sources::{SourceAdapter, SourceAsset};
use crate::store::{CacheEntry, CachePolicy, ContentAddressedStore};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct SourceResolutionRequest {
    pub(super) adapter: Arc<dyn SourceAdapter>,
    pub(super) id_or_url: String,
}

#[derive(Clone)]
pub(super) struct DownloadHttpRequest {
    pub(super) source: String,
    pub(super) asset: SourceAsset,
    pub(super) cache_policy: CachePolicy,
    pub(super) redirect_policy: RedirectPolicy,
    pub(super) force_download: bool,
    pub(super) cached: Option<CacheEntry>,
}

impl DownloadHttpRequest {
    pub(super) fn download_request(&self) -> AssetDownloadRequest<'_> {
        AssetDownloadRequest {
            source: self.source.as_str(),
            asset: &self.asset,
            cache_policy: self.cache_policy,
            redirect_policy: self.redirect_policy,
            force: self.force_download,
        }
    }
}

pub(super) async fn resolve_source_record_for_job(
    request: SourceResolutionRequest,
) -> Result<ResolvedSourceRecord, SourceResolutionError> {
    resolve_source_record(request.adapter.as_ref(), request.id_or_url.as_str()).await
}

pub(super) async fn download_asset_http_for_job(
    downloader: &AssetDownloader,
    files: &ContentAddressedStore,
    request: DownloadHttpRequest,
) -> DownloadResult<PendingCachePersist> {
    let DownloadHttpRequest {
        source,
        asset,
        cache_policy,
        redirect_policy,
        force_download,
        cached,
    } = request;
    let download_request = AssetDownloadRequest {
        source: source.as_str(),
        asset: &asset,
        cache_policy,
        redirect_policy,
        force: force_download,
    };
    downloader
        .download_http(files, download_request, cached)
        .await
}
