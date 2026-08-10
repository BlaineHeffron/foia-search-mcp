use rmcp::{model::*, ErrorData as McpError};
use serde::Serialize;

use crate::errors::FoiaSearchError;

pub fn json_result<T>(value: &T) -> Result<CallToolResult, McpError>
where
    T: Serialize,
{
    let json = serde_json::to_string_pretty(value)
        .map_err(FoiaSearchError::from)
        .map_err(FoiaSearchError::into_mcp_error)?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
}
