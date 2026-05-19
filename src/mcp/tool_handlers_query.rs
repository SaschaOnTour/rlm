//! MCP read-side query tool handlers: `search`, `overview`, `refs`, `files`.

use std::path::Path;

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::facades;
use crate::application::query::files::FilesFilter;
use crate::application::query::search::FieldsMode;
use crate::application::query::DetailLevel;
use crate::error::RlmError;
use crate::output::Formatter;

use super::server::RlmServer;

/// Parse the MCP tool's `fields` JSON param to `FieldsMode`. Lives at
/// the MCP boundary because clap-driven CLI parsing uses a different
/// path (the `FieldsArg` enum + `From<FieldsArg>` conversion); this
/// helper is JSON-string-driven and adapter-specific.
fn parse_fields_mode(raw: Option<&str>) -> crate::error::Result<FieldsMode> {
    match raw {
        None => Ok(FieldsMode::default()),
        Some("full") => Ok(FieldsMode::Full),
        Some("minimal") => Ok(FieldsMode::Minimal),
        Some(other) => Err(RlmError::InvalidPattern {
            pattern: other.to_string(),
            reason: "unknown fields mode — use 'full' or 'minimal'".into(),
        }),
    }
}

/// Parse the MCP tool's `detail` JSON param to `DetailLevel`. Same
/// rationale as [`parse_fields_mode`]: CLI uses a clap-derived enum,
/// MCP receives a raw JSON string and parses it here.
fn parse_detail_level(raw: Option<&str>) -> crate::error::Result<DetailLevel> {
    match raw {
        None => Ok(DetailLevel::default()),
        Some("minimal") => Ok(DetailLevel::Minimal),
        Some("standard") => Ok(DetailLevel::Standard),
        Some("tree") => Ok(DetailLevel::Tree),
        Some(other) => Err(RlmError::InvalidPattern {
            pattern: other.to_string(),
            reason: "unknown detail level — use 'minimal', 'standard', or 'tree'".into(),
        }),
    }
}

/// Handle the `search` tool: full-text search across indexed chunks.
pub fn handle_search(
    project_root: &Path,
    query: &str,
    limit: usize,
    fields: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let mode = match parse_fields_mode(fields) {
        Ok(m) => m,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };
    RlmServer::respond_string(
        formatter,
        facades::search_project(project_root, query, limit, mode).map(|r| r.body),
    )
}

/// Handle the `overview` tool: project structure at three detail
/// levels. The detail string comes from the JSON payload — we parse
/// it at the adapter boundary so the session receives a typed value.
pub fn handle_overview(
    project_root: &Path,
    detail: Option<&str>,
    path: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let level = match parse_detail_level(detail) {
        Ok(l) => l,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };
    RlmServer::respond_string(
        formatter,
        facades::overview_project(project_root, level, path).map(|r| r.body),
    )
}

/// Handle the `refs` tool: find all usages of a symbol with impact analysis.
pub fn handle_refs(
    project_root: &Path,
    symbol: &str,
    parent: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_string(
        formatter,
        facades::refs_project(project_root, symbol, parent).map(|r| r.body),
    )
}

/// Handle the `files` tool: list all project files. `files` works
/// even when no index exists (it scans the filesystem directly), so
/// this handler doesn't require an open session.
pub fn handle_files(
    project_root: &Path,
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
    RlmServer::respond_json(
        formatter,
        crate::application::query::files::list_files(project_root, filter),
    )
}

#[cfg(test)]
#[path = "tool_handlers_query_tests.rs"]
mod tests;
