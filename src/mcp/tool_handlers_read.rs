//! MCP `read` tool handler: symbol / section retrieval.
//!
//! The business logic (chunk-by-ident, file/parent filtering,
//! metadata enrichment, section lookup with helpful "not found"
//! hints) lives in [`crate::application::query::read`]. This handler
//! only translates the MCP request shape into a typed
//! [`ReadRequest`] and routes the result through the response
//! envelope.

use std::path::Path;

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::facades;
use crate::application::query::read::ReadInputs;
use crate::output::Formatter;

use super::server::RlmServer;
use super::tools::ReadParams;

/// Handle the `read` tool: read a specific symbol or markdown section.
pub fn handle_read(
    project_root: &Path,
    params: &ReadParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let inputs = ReadInputs {
        path: &params.path,
        symbol: params.symbol.as_deref(),
        section: params.section.as_deref(),
        parent: params.parent.as_deref(),
        metadata: params.metadata.unwrap_or(false),
    };
    RlmServer::respond_string(
        formatter,
        facades::read_project(project_root, &inputs).map(|out| out.body),
    )
}
