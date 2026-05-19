//! Utility tool handlers for the MCP server.
//!
//! Each handler is a thin wrapper over a single
//! [`application::facades`](crate::application::facades) call — one
//! application-touchpoint per handler, which is what call_parity
//! enforces.

use std::path::Path;

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::content::partition;
use crate::application::facades;
use crate::application::query::stats::QualityFlags;
use crate::application::session::RlmSession;
use crate::output::Formatter;

use super::server::RlmServer;

/// Handle the `stats` tool: indexing summary or token-savings report.
pub fn handle_stats(
    project_root: &Path,
    savings_flag: bool,
    since: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_json(
        formatter,
        facades::stats_project(project_root, savings_flag, since).map(|out| out.body),
    )
}

/// Handle the `quality` tool: inspect parse-quality issues.
pub fn handle_quality(
    project_root: &Path,
    flags: QualityFlags,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_json(formatter, facades::quality_project(project_root, flags))
}

/// Handle the `partition` tool: split a file into chunks.
pub fn handle_partition(
    project_root: &Path,
    path: &str,
    strategy_str: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let strategy: partition::Strategy = match strategy_str.parse() {
        Ok(s) => s,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };
    RlmServer::respond_string(
        formatter,
        facades::partition_project(project_root, path, strategy).map(|r| r.body),
    )
}

/// Handle the `summarize` tool: generate a condensed file summary.
pub fn handle_summarize(
    project_root: &Path,
    path: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_string(
        formatter,
        facades::summarize_project(project_root, path).map(|r| r.body),
    )
}

/// Handle the `diff` tool: compare indexed vs disk version.
pub fn handle_diff(
    project_root: &Path,
    path: &str,
    symbol: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_string(
        formatter,
        facades::diff_project(project_root, path, symbol).map(|r| r.body),
    )
}

/// Handle the `context` tool: complete understanding of a symbol.
pub fn handle_context(
    project_root: &Path,
    symbol: &str,
    include_graph: bool,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_string(
        formatter,
        facades::context_project(project_root, symbol, include_graph).map(|r| r.body),
    )
}

/// Handle the `deps` tool: file dependency analysis.
pub fn handle_deps(
    project_root: &Path,
    path: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_string(
        formatter,
        facades::deps_project(project_root, path).map(|r| r.body),
    )
}

/// Handle the `scope` tool: symbols visible at a specific line.
pub fn handle_scope(
    project_root: &Path,
    path: &str,
    line: u32,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_string(
        formatter,
        facades::scope_project(project_root, path, line).map(|r| r.body),
    )
}

/// Handle the `verify` tool: verify index integrity.
pub fn handle_verify(
    project_root: &Path,
    fix: bool,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    RlmServer::respond_json(formatter, facades::verify_project(project_root, fix))
}

/// Handle the `supported` tool: list supported file extensions.
pub fn handle_supported(formatter: Formatter) -> Result<CallToolResult, McpError> {
    Ok(RlmServer::success_text(
        formatter,
        RlmServer::to_json(&RlmSession::supported()),
    ))
}
