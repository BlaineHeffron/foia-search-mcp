use super::{
    aaro::AaroAdapter, cia::CiaAdapter, doj_epstein::DojEpsteinAdapter, doj_foia::DojFoiaAdapter,
    fbi_vault::FbiVaultAdapter, govinfo::GovInfoAdapter, nara::NaraAdapter, pursue::PursueAdapter,
    CachePolicy, SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture,
    SourceRecord, SourceStatus,
};

struct DummyAdapter;

impl SourceAdapter for DummyAdapter {
    fn name(&self) -> &'static str {
        "dummy"
    }

    fn status(&self) -> SourceStatus {
        SourceStatus::Enabled
    }

    fn search<'a>(
        &'a self,
        _query: &'a str,
        _options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async { Err(SourceError::invalid_input("dummy", "unused", None)) })
    }

    fn get_record<'a>(&'a self, _id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async { Err(SourceError::invalid_input("dummy", "unused", None)) })
    }

    fn list_assets<'a>(&'a self, _record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async { Err(SourceError::invalid_input("dummy", "unused", None)) })
    }
}

#[test]
fn source_adapter_default_cache_policy_is_respect_source_headers() {
    let adapter = DummyAdapter;
    assert_eq!(adapter.cache_policy(), CachePolicy::RespectSourceHeaders);
}

#[test]
fn aaro_adapter_cache_policy_remains_respect_source_headers() {
    let adapter = AaroAdapter::default();
    assert_eq!(adapter.cache_policy(), CachePolicy::RespectSourceHeaders);
}

#[test]
fn cia_adapter_cache_policy_remains_respect_source_headers() {
    let adapter = CiaAdapter::default();
    assert_eq!(adapter.cache_policy(), CachePolicy::RespectSourceHeaders);
}

#[test]
fn nara_adapter_cache_policy_remains_do_not_persist() {
    let adapter = NaraAdapter::default();
    assert_eq!(adapter.cache_policy(), CachePolicy::DoNotPersist);
}

#[test]
fn govinfo_adapter_cache_policy_remains_respect_source_headers() {
    let adapter = GovInfoAdapter::default();
    assert_eq!(adapter.cache_policy(), CachePolicy::RespectSourceHeaders);
}

#[test]
fn pursue_adapter_cache_policy_remains_respect_source_headers() {
    let adapter = PursueAdapter::default();
    assert_eq!(adapter.cache_policy(), CachePolicy::RespectSourceHeaders);
}

#[test]
fn doj_epstein_adapter_cache_policy_remains_respect_source_headers() {
    let adapter = DojEpsteinAdapter::default();
    assert_eq!(adapter.cache_policy(), CachePolicy::RespectSourceHeaders);
}

#[test]
fn doj_foia_adapter_cache_policy_remains_respect_source_headers() {
    let adapter = DojFoiaAdapter::default();
    assert_eq!(adapter.cache_policy(), CachePolicy::RespectSourceHeaders);
}

#[test]
fn fbi_vault_adapter_cache_policy_remains_respect_source_headers() {
    let adapter = FbiVaultAdapter::default();
    assert_eq!(adapter.cache_policy(), CachePolicy::RespectSourceHeaders);
}
