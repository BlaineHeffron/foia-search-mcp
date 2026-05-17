use crate::ingest::executor_async::{
    download_asset_http_for_job, resolve_source_record_for_job, DownloadHttpRequest,
    SourceResolutionRequest,
};
use crate::ingest::AssetDownloader;
use crate::sources::{
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceAssetRole,
    SourceFuture, SourceMetadata, SourceRecord, SourceStatus,
};
use crate::store::ContentAddressedStore;
use std::sync::Arc;

fn assert_send<T: Send>(_: T) {}

#[derive(Clone)]
struct FakeAdapter {
    record: SourceRecord,
}

impl SourceAdapter for FakeAdapter {
    fn name(&self) -> &'static str {
        "cia"
    }

    fn status(&self) -> SourceStatus {
        SourceStatus::Enabled
    }

    fn search<'a>(
        &'a self,
        _query: &'a str,
        _options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move {
            Ok(SearchPage {
                query: String::new(),
                source: "cia",
                records: vec![self.record.clone()],
                next_cursor: None,
                warnings: Vec::new(),
            })
        })
    }

    fn get_record<'a>(&'a self, _id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move { Ok(self.record.clone()) })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(record.attachments.clone()) })
    }

    fn cache_policy(&self) -> CachePolicy {
        CachePolicy::RespectSourceHeaders
    }
}

#[test]
fn async_store_free_executor_boundaries_are_send() {
    let adapter = Arc::new(FakeAdapter {
        record: source_record("https://example.test/fixture.pdf".to_owned()),
    });
    let source_request = SourceResolutionRequest {
        adapter,
        id_or_url: "CREST-executor".to_owned(),
    };
    assert_send(resolve_source_record_for_job(source_request));

    let downloader = AssetDownloader::new().expect("downloader");
    let files_dir = tempfile::tempdir().expect("tempdir");
    let files = ContentAddressedStore::new(files_dir.path());
    let download_request = DownloadHttpRequest {
        source: "cia".to_owned(),
        asset: SourceAsset {
            asset_url: "https://example.test/fixture.pdf".to_owned(),
            label: "PDF".to_owned(),
            mime_type: Some("application/pdf".to_owned()),
            role: SourceAssetRole::Pdf,
        },
        cache_policy: crate::store::CachePolicy::RespectSourceHeaders,
        redirect_policy: crate::ingest::RedirectPolicy::Deny,
        force_download: false,
        cached: None,
    };
    assert_send(download_asset_http_for_job(
        &downloader,
        &files,
        download_request,
    ));
}

fn source_record(asset_url: String) -> SourceRecord {
    SourceRecord {
        id: "cia:CREST-executor".to_owned(),
        document_key: "cia_CREST-executor".to_owned(),
        source: "cia",
        source_id: "CREST-executor".to_owned(),
        title: "Executor Fixture".to_owned(),
        date: None,
        collection: Some("CREST".to_owned()),
        record_group: None,
        description: Some("executor test".to_owned()),
        origin_url: "https://www.cia.gov/readingroom/document/CREST-executor".to_owned(),
        document_url: "https://www.cia.gov/readingroom/document/CREST-executor".to_owned(),
        pdf_url: Some(asset_url.clone()),
        metadata: SourceMetadata::new(),
        attachments: vec![SourceAsset {
            asset_url,
            label: "PDF".to_owned(),
            mime_type: Some("application/pdf".to_owned()),
            role: SourceAssetRole::Pdf,
        }],
        text_preview: None,
        citation_note: Some("cite source".to_owned()),
        terms_note: Some("terms".to_owned()),
    }
}
