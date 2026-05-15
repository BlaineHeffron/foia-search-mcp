use rmcp::ErrorData as McpError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FoiaSearchError {
    #[error("source adapter is not configured: {adapter}")]
    SourceUnavailable { adapter: String },

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl FoiaSearchError {
    pub fn into_mcp_error(self) -> McpError {
        match self {
            Self::InvalidRequest(message) => McpError::invalid_params(message, None),
            other => McpError::internal_error(other.to_string(), None),
        }
    }
}
