use super::{
    aaro::AaroAdapter, cia::CiaAdapter, doj_epstein::DojEpsteinAdapter, doj_foia::DojFoiaAdapter,
    dtic::DticAdapter, fbi_vault::FbiVaultAdapter, frus::FrusAdapter, govinfo::GovInfoAdapter,
    nara::NaraAdapter, noaa::NoaaAdapter, pursue::PursueAdapter, CachePolicy, SearchOptions,
    SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord, SourceStatus,
};
use crate::ingest::RedirectPolicy;

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

    fn cache_policy(&self) -> CachePolicy {
        CachePolicy::DoNotPersist
    }
}

#[test]
fn source_adapter_default_redirect_policy_is_deny() {
    let adapter = DummyAdapter;
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn aaro_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = AaroAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn cia_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = CiaAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn nara_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = NaraAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn govinfo_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = GovInfoAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn pursue_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = PursueAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn doj_epstein_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = DojEpsteinAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn doj_foia_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = DojFoiaAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn fbi_vault_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = FbiVaultAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn frus_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = FrusAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn dtic_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = DticAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}

#[test]
fn noaa_adapter_redirect_policy_remains_deny_by_default() {
    let adapter = NoaaAdapter::default();
    assert_eq!(adapter.redirect_policy(), RedirectPolicy::Deny);
}
