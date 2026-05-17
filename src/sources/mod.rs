pub mod cia;
pub mod govinfo;
pub mod nara;

use crate::ingest::RedirectPolicy;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

pub type SourceMetadata = BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CachePolicy {
    RespectSourceHeaders,
    DoNotPersist,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOptions {
    pub max_results: usize,
    pub cursor: Option<String>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_results: 10,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    pub query: String,
    pub source: &'static str,
    pub records: Vec<SourceRecord>,
    pub next_cursor: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    /// User-facing stable id. Do not use this as a filesystem path.
    pub id: String,
    /// Filesystem-safe internal key for derived artifacts.
    pub document_key: String,
    pub source: &'static str,
    pub source_id: String,
    pub title: String,
    pub date: Option<String>,
    pub collection: Option<String>,
    pub record_group: Option<String>,
    pub description: Option<String>,
    pub origin_url: String,
    pub document_url: String,
    pub pdf_url: Option<String>,
    pub metadata: SourceMetadata,
    pub attachments: Vec<SourceAsset>,
    pub text_preview: Option<String>,
    pub citation_note: Option<String>,
    pub terms_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAsset {
    pub asset_url: String,
    pub label: String,
    pub mime_type: Option<String>,
    pub role: SourceAssetRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceAssetRole {
    Pdf,
    Html,
    OcrText,
    Transcript,
    Image,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    InvalidInput {
        source: &'static str,
        message: String,
        guidance: Option<String>,
    },
    SourceChanged {
        source: &'static str,
        message: String,
        url: Option<String>,
    },
    Fetch {
        source: &'static str,
        message: String,
        url: Option<String>,
    },
}

impl SourceError {
    pub fn invalid_input(
        source: &'static str,
        message: impl Into<String>,
        guidance: Option<String>,
    ) -> Self {
        Self::InvalidInput {
            source,
            message: message.into(),
            guidance,
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::InvalidInput {
                source,
                message,
                guidance,
            } => {
                write!(f, "{source}: {message}")?;
                if let Some(guidance) = guidance {
                    write!(f, " Guidance: {guidance}")?;
                }
                Ok(())
            }
            SourceError::SourceChanged {
                source,
                message,
                url,
            } => {
                write!(f, "{source}: {message}")?;
                if let Some(url) = url {
                    write!(f, " Manual source URL: {url}")?;
                }
                Ok(())
            }
            SourceError::Fetch {
                source,
                message,
                url,
            } => {
                write!(f, "{source}: {message}")?;
                if let Some(url) = url {
                    write!(f, " URL: {url}")?;
                }
                Ok(())
            }
        }
    }
}

impl Error for SourceError {}

pub type SourceFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SourceError>> + Send + 'a>>;

pub trait SourceAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn status(&self) -> SourceStatus;
    fn search<'a>(&'a self, query: &'a str, options: SearchOptions)
        -> SourceFuture<'a, SearchPage>;
    fn get_record<'a>(&'a self, id_or_url: &'a str) -> SourceFuture<'a, SourceRecord>;
    fn list_assets<'a>(&'a self, record: &'a SourceRecord) -> SourceFuture<'a, Vec<SourceAsset>>;
    fn cache_policy(&self) -> CachePolicy {
        CachePolicy::RespectSourceHeaders
    }
    fn redirect_policy(&self) -> RedirectPolicy {
        RedirectPolicy::Deny
    }
}

#[cfg(test)]
mod cache_policy_contract_tests;

#[cfg(test)]
mod redirect_contract_tests;
