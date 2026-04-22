//! Utility tool handlers for the MCP server.
//!
//! Contains handlers for utility/diagnostic tools: stats, quality,
//! partition, summarize, diff, context, deps, scope, verify, supported.
//! Every handler is a thin wrapper over one [`RlmSession`] method.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::content::partition;
use crate::application::query::stats::QualityFlags;
use crate::application::session::RlmSession;
use crate::output::Formatter;

use super::server::RlmServer;

/// Handle the `stats` tool: indexing summary or token-savings report.
// qual:api
pub fn handle_stats(
    session: &RlmSession,
    savings_flag: bool,
    since: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.stats(savings_flag, since) {
        Ok(out) => Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&out.body),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `quality` tool: inspect parse-quality issues.
// qual:api
pub fn handle_quality(
    session: &RlmSession,
    flags: QualityFlags,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.quality(flags) {
        Ok(body) => Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&body),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `partition` tool: split a file into chunks.
// qual:api
pub fn handle_partition(
    session: &RlmSession,
    path: &str,
    strategy_str: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let strategy: partition::Strategy = match strategy_str.parse() {
        Ok(s) => s,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };
    match session.partition(path, strategy) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `summarize` tool: generate a condensed file summary.
// qual:api
pub fn handle_summarize(
    session: &RlmSession,
    path: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.summarize(path) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `diff` tool: compare indexed vs disk version.
// qual:api
pub fn handle_diff(
    session: &RlmSession,
    path: &str,
    symbol: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.diff(path, symbol) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `context` tool: complete understanding of a symbol.
// qual:api
pub fn handle_context(
    session: &RlmSession,
    symbol: &str,
    include_graph: bool,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.context(symbol, include_graph) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `deps` tool: file dependency analysis.
// qual:api
pub fn handle_deps(
    session: &RlmSession,
    path: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.deps(path) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `scope` tool: symbols visible at a specific line.
// qual:api
pub fn handle_scope(
    session: &RlmSession,
    path: &str,
    line: u32,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.scope(path, line) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `verify` tool: verify index integrity.
// qual:api
pub fn handle_verify(
    session: &RlmSession,
    fix: bool,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.verify(fix) {
        Ok(result) => Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&result),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `supported` tool: list supported file extensions.
// qual:api
pub fn handle_supported(formatter: Formatter) -> Result<CallToolResult, McpError> {
    Ok(RlmServer::success_text(
        formatter,
        RlmServer::to_json(&RlmSession::supported()),
    ))
}
