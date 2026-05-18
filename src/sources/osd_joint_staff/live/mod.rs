use crate::http::fetch_text;
use crate::sources::{
    SearchOptions, SearchPage, SourceAdapter, SourceAsset, SourceError, SourceFuture, SourceRecord,
    SourceStatus,
};

mod asset;
mod html;
mod parse;
mod url;

use asset::{asset_priority_key, dedupe_assets, is_direct_download_url};
use parse::{
    record_from_detail_page, record_from_direct_asset_url, record_matches_query,
    records_from_reading_room_page,
};
use url::{canonicalize_official_url, detail_url_from_source_id, is_allowed_osd_joint_staff_url};

pub const OSD_JOINT_STAFF_SOURCE: &str = "osd_joint_staff";
pub const OSD_JOINT_STAFF_SEARCH_SOURCE: &str = "osd_joint_staff_foia_reading_room";
pub const OSD_JOINT_STAFF_BASE_URL: &str = "https://www.esd.whs.mil";
pub const OSD_JOINT_STAFF_READING_ROOM_PATH: &str =
    "/Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/";

const SOURCE_CHANGED_WARNING: &str = "OSD/Joint Staff FOIA Reading Room format may have changed. Verify official WHS/ESD OSD/Joint Staff FOIA pages manually.";
const CITATION_NOTE: &str = "Official OSD/Joint Staff FOIA Reading Room lead from WHS/ESD. Cite the official www.esd.whs.mil page and linked PDF URL, and verify PDF page boundaries, OCR quality, redactions, and originating context before publication.";
const TERMS_NOTE: &str = "Use official www.esd.whs.mil OSD/Joint Staff FOIA Reading Room pages for research leads. Avoid mirrors and bulk scraping; page-level citation requires PDF ingestion and boundary verification.";

#[derive(Debug, Clone)]
pub struct OsdJointStaffAdapter {
    base_url: String,
}

enum OsdJointStaffLocator {
    Url(String),
    SourceId(String),
}

impl OsdJointStaffAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("FOIA_SEARCH_OSD_JOINT_STAFF_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Self::new)
            .unwrap_or_default()
    }

    fn base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        if trimmed.is_empty() {
            OSD_JOINT_STAFF_BASE_URL.to_owned()
        } else {
            trimmed.trim_end_matches('/').to_owned()
        }
    }

    fn reading_room_url(&self) -> String {
        format!("{}{}", self.base_url(), OSD_JOINT_STAFF_READING_ROOM_PATH)
    }

    fn parse_locator(&self, id_or_url: &str) -> Result<OsdJointStaffLocator, SourceError> {
        let mut value = id_or_url.trim();
        if value.is_empty() {
            return Err(SourceError::invalid_input(
                OSD_JOINT_STAFF_SOURCE,
                "OSD/Joint Staff lookup expects a non-empty source id or official OSD/Joint Staff FOIA URL.",
                    Some(
                    "Examples: osd_joint_staff:Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff or https://www.esd.whs.mil/Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff/"
                        .to_owned(),
                ),
            ));
        }

        if let Some(stripped) = value.strip_prefix("osd_joint_staff:") {
            value = stripped.trim();
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            let normalized = canonicalize_official_url(value, &self.base_url());
            if !is_allowed_osd_joint_staff_url(&normalized, &self.base_url()) {
                return Err(SourceError::invalid_input(
                    OSD_JOINT_STAFF_SOURCE,
                    "OSD/Joint Staff lookup only accepts official same-origin www.esd.whs.mil FOIA URLs.",
                    Some("Use URLs rooted at https://www.esd.whs.mil/FOID/ or https://www.esd.whs.mil/Records-Declass/FOIA/Reading-Room/.".to_owned()),
                ));
            }
            return Ok(OsdJointStaffLocator::Url(normalized));
        }

        Ok(OsdJointStaffLocator::SourceId(value.to_owned()))
    }

    async fn search_records(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<SearchPage, SourceError> {
        let reading_room_url = self.reading_room_url();
        let html = fetch_text(OSD_JOINT_STAFF_SOURCE, &reading_room_url).await?;
        let mut records =
            records_from_reading_room_page(&html, &self.base_url(), &reading_room_url);
        let joint_staff_url = format!(
            "{}{}Joint_Staff/",
            self.base_url(),
            OSD_JOINT_STAFF_READING_ROOM_PATH
        );
        let joint_staff_html = fetch_text(OSD_JOINT_STAFF_SOURCE, &joint_staff_url).await?;
        records.extend(records_from_reading_room_page(
            &joint_staff_html,
            &self.base_url(),
            &joint_staff_url,
        ));
        records.retain(|record| record_matches_query(record, query));
        records.truncate(max_results);

        let warnings = if records.is_empty() {
            vec![
                "OSD/Joint Staff FOIA Reading Room returned no matching official leads. Try broader terms or review the official www.esd.whs.mil FOIA Reading Room manually."
                    .to_owned(),
            ]
        } else {
            Vec::new()
        };

        Ok(SearchPage {
            query: query.to_owned(),
            source: OSD_JOINT_STAFF_SEARCH_SOURCE,
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn get_record_by_url(&self, url: &str) -> Result<SourceRecord, SourceError> {
        let normalized = canonicalize_official_url(url, &self.base_url());
        if !is_allowed_osd_joint_staff_url(&normalized, &self.base_url()) {
            return Err(SourceError::invalid_input(
                OSD_JOINT_STAFF_SOURCE,
                "OSD/Joint Staff lookup only accepts official same-origin www.esd.whs.mil FOIA URLs.",
                Some("Use official www.esd.whs.mil OSD/Joint Staff FOIA Reading Room URLs.".to_owned()),
            ));
        }
        if is_direct_download_url(&normalized) {
            return Ok(record_from_direct_asset_url(&normalized, &self.base_url()));
        }

        let html = fetch_text(OSD_JOINT_STAFF_SOURCE, &normalized).await?;
        record_from_detail_page(&html, &self.base_url(), &normalized, None).ok_or_else(|| {
            SourceError::SourceChanged {
                source: OSD_JOINT_STAFF_SOURCE,
                message: SOURCE_CHANGED_WARNING.to_owned(),
                url: Some(normalized),
            }
        })
    }

    async fn get_record_by_source_id(&self, source_id: &str) -> Result<SourceRecord, SourceError> {
        let Some(url) = detail_url_from_source_id(source_id, &self.base_url()) else {
            return Err(SourceError::invalid_input(
                OSD_JOINT_STAFF_SOURCE,
                    "OSD/Joint Staff source_id format is not recognized.",
                    Some(
                    "Use ids such as Records-Declass/FOIA/Reading-Room/Reading-Room-List_2/Joint_Staff or osd_joint_staff:<official-path>."
                        .to_owned(),
                ),
            ));
        };

        self.get_record_by_url(&url).await
    }
}

impl Default for OsdJointStaffAdapter {
    fn default() -> Self {
        Self::new(OSD_JOINT_STAFF_BASE_URL)
    }
}

impl SourceAdapter for OsdJointStaffAdapter {
    fn name(&self) -> &'static str {
        OSD_JOINT_STAFF_SOURCE
    }

    fn status(&self) -> SourceStatus {
        SourceStatus::Enabled
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        options: SearchOptions,
    ) -> SourceFuture<'a, SearchPage> {
        Box::pin(async move {
            let query = query.trim();
            if query.is_empty() {
                return Err(SourceError::invalid_input(
                    OSD_JOINT_STAFF_SOURCE,
                    "OSD/Joint Staff FOIA Reading Room search expects a non-empty query string.",
                    Some("Try terms such as 'Joint Staff', 'Long War', 'detainee', or 'national military strategy'.".to_owned()),
                ));
            }

            self.search_records(query, options.max_results).await
        })
    }

    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord> {
        Box::pin(async move {
            match self.parse_locator(id_or_url)? {
                OsdJointStaffLocator::Url(url) => self.get_record_by_url(&url).await,
                OsdJointStaffLocator::SourceId(source_id) => {
                    self.get_record_by_source_id(&source_id).await
                }
            }
        })
    }

    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>> {
        Box::pin(async move {
            let mut assets = dedupe_assets(record.attachments.clone());
            assets.sort_by_key(asset_priority_key);
            Ok(assets)
        })
    }
}

pub fn osd_joint_staff_citation_note() -> &'static str {
    CITATION_NOTE
}

pub fn osd_joint_staff_terms_note() -> &'static str {
    TERMS_NOTE
}
