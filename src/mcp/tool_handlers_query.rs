//! MCP read-side query tool handlers: `search`, `overview`, `refs`, `files`.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::query::files::FilesFilter;
use crate::application::query::search::FieldsMode;
use crate::application::query::DetailLevel;
use crate::application::session::RlmSession;
use crate::output::Formatter;

use super::server::RlmServer;

/// Handle the `search` tool: full-text search across indexed chunks.
// qual:api
pub fn handle_search(
    session: &RlmSession,
    query: &str,
    limit: usize,
    fields: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let mode = match FieldsMode::from_optional(fields) {
        Ok(m) => m,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };
    match session.search(query, limit, mode) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `overview` tool: project structure at three detail
/// levels. The detail string comes from the JSON payload — we parse
/// it at the adapter boundary so the session receives a typed value.
// qual:api
pub fn handle_overview(
    session: &RlmSession,
    detail: Option<&str>,
    path: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let level = match DetailLevel::from_optional(detail) {
        Ok(l) => l,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };
    match session.overview(level, path) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `refs` tool: find all usages of a symbol with impact analysis.
// qual:api
pub fn handle_refs(
    session: &RlmSession,
    symbol: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.refs(symbol) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `files` tool: list all project files. `files` works
/// even when no index exists (it scans the filesystem directly), so
/// this handler doesn't require an open session.
// qual:api
pub fn handle_files(
    project_root: &std::path::Path,
    path_prefix: Option<String>,
    skipped_only: bool,
    indexed_only: bool,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let filter = FilesFilter {
        path_prefix,
        skipped_only,
        indexed_only,
    };
    match crate::application::query::files::list_files(project_root, filter) {
        Ok(result) => Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&result),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}
