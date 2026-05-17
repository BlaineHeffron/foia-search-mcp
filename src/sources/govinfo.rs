use super::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

pub const GOVINFO_SOURCE: &str = "govinfo";
pub const GOVINFO_SEARCH_SOURCE: &str = "govinfo_search_service";
pub const GOVINFO_API_ROOT: &str = "https://api.govinfo.gov";
pub const GOVINFO_SEARCH_OVERVIEW_URL: &str =
    "https://www.govinfo.gov/features/search-service-overview";

const SEARCH_MANUAL_MESSAGE: &str =
    "GovInfo adapter tracer is present but live GovInfo API search is not wired in this slice.";
const RECORD_MANUAL_MESSAGE: &str = "GovInfo record fetch is not wired in this tracer slice.";
const CITATION_NOTE: &str =
    "GovInfo publication metadata. Verify package/granule links and cited pages in the official publication.";
const TERMS_NOTE: &str =
    "Use official GovInfo API search/package/granule endpoints and prefer PDF/XML/MODS links over HTML scraping.";

#[derive(Debug, Clone)]
pub struct GovInfoAdapter {
    api_root: String,
}

impl GovInfoAdapter {
    pub fn new(api_root: impl Into<String>) -> Self {
        Self {
            api_root: api_root.into(),
        }
    }

    pub fn api_root(&self) -> &str {
        &self.api_root
    }

    pub fn search_endpoint(&self) -> String {
        format!("{}/search", self.api_root.trim_end_matches('/'))
    }

    fn manual_search_guidance(&self) -> String {
        format!(
            "Use the official GovInfo Search Service POST endpoint ({}) and continue pagination with offsetMark. Prefer package/granule download links (pdfLink/xmlLink/modsLink) for ingestion and citation. Reference: {}",
            self.search_endpoint(),
            GOVINFO_SEARCH_OVERVIEW_URL
        )
    }

    fn manual_record_guidance(&self) -> String {
        let root = self.api_root.trim_end_matches('/');
        format!(
            "Resolve package/granule summaries manually using official endpoints such as {root}/packages/{{PACKAGE_ID}}/summary and {root}/packages/{{PACKAGE_ID}}/granules/{{GRANULE_ID}}/summary. Prefer API PDF/XML/MODS links over HTML pages. Reference: {GOVINFO_SEARCH_OVERVIEW_URL}"
        )
    }
}

impl Default for GovInfoAdapter {
    fn default() -> Self {
        Self::new(GOVINFO_API_ROOT)
    }
}

impl SourceAdapter for GovInfoAdapter {
    fn name(&self) -> &'static str {
        GOVINFO_SOURCE
    }

    fn status(&self) -> SourceStatus {
        SourceStatus::Disabled
    }

    fn search<'a>(
        &'a self,
        _query: &'a str,
        _options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move {
            Err(SourceError::invalid_input(
                GOVINFO_SOURCE,
                SEARCH_MANUAL_MESSAGE,
                Some(self.manual_search_guidance()),
            ))
        })
    }

    fn get_record<'a>(&'a self, _id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            Err(SourceError::invalid_input(
                GOVINFO_SOURCE,
                RECORD_MANUAL_MESSAGE,
                Some(self.manual_record_guidance()),
            ))
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move { Ok(record.attachments.clone()) })
    }
}

pub fn govinfo_terms_note() -> &'static str {
    TERMS_NOTE
}

pub fn govinfo_citation_note() -> &'static str {
    CITATION_NOTE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{SourceAssetRole, SourceMetadata};

    #[tokio::test]
    async fn tracer_stays_disabled() {
        let adapter = GovInfoAdapter::default();
        assert_eq!(adapter.name(), GOVINFO_SOURCE);
        assert_eq!(adapter.status(), SourceStatus::Disabled);
    }

    #[tokio::test]
    async fn search_returns_manual_guidance() {
        let adapter = GovInfoAdapter::default();
        let err = adapter
            .search("hearing climate", SearchOptions::default())
            .await
            .expect_err("search tracer should remain manual for now");

        match err {
            SourceError::InvalidInput {
                source,
                message,
                guidance,
            } => {
                assert_eq!(source, GOVINFO_SOURCE);
                assert!(message.contains("not wired"));
                let guidance = guidance.expect("manual guidance should be present");
                assert!(guidance.contains("/search"));
                assert!(guidance.contains("offsetMark"));
                assert!(guidance.contains("pdfLink/xmlLink/modsLink"));
                assert!(guidance.contains(GOVINFO_SEARCH_OVERVIEW_URL));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_record_returns_manual_package_granule_guidance() {
        let adapter = GovInfoAdapter::default();
        let err = adapter
            .get_record("CREC-2024-01-01")
            .await
            .expect_err("record tracer should remain manual for now");

        match err {
            SourceError::InvalidInput {
                source,
                message,
                guidance,
            } => {
                assert_eq!(source, GOVINFO_SOURCE);
                assert!(message.contains("not wired"));
                let guidance = guidance.expect("manual guidance should be present");
                assert!(guidance.contains("/packages/{PACKAGE_ID}/summary"));
                assert!(guidance.contains("/granules/{GRANULE_ID}/summary"));
                assert!(guidance.contains("PDF/XML/MODS"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_assets_reuses_record_attachments() {
        let adapter = GovInfoAdapter::default();
        let record = SourceRecord {
            id: "govinfo:pkg-test".to_owned(),
            document_key: "doc-key".to_owned(),
            source: GOVINFO_SOURCE,
            source_id: "pkg-test".to_owned(),
            title: "Package".to_owned(),
            date: None,
            collection: None,
            record_group: None,
            description: None,
            origin_url: "https://www.govinfo.gov/app/details/PKG".to_owned(),
            document_url: "https://www.govinfo.gov/app/details/PKG".to_owned(),
            pdf_url: Some("https://api.govinfo.gov/packages/PKG/pdf".to_owned()),
            metadata: SourceMetadata::new(),
            attachments: vec![SourceAsset {
                asset_url: "https://api.govinfo.gov/packages/PKG/pdf".to_owned(),
                label: "Package PDF".to_owned(),
                mime_type: Some("application/pdf".to_owned()),
                role: SourceAssetRole::Pdf,
            }],
            text_preview: None,
            citation_note: Some(govinfo_citation_note().to_owned()),
            terms_note: Some(govinfo_terms_note().to_owned()),
        };

        let assets = adapter
            .list_assets(&record)
            .await
            .expect("asset list should clone attachments");
        assert_eq!(assets, record.attachments);
    }
}
