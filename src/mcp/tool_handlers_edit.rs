//! MCP write-side tool handlers: `replace`, `delete`, `insert`, `extract`.
//!
//! Every handler parses the rmcp [`Parameters`] into an
//! application-layer input struct, calls one [`RlmSession`] method,
//! and emits the result via [`RlmServer`]. All orchestration
//! (op → reindex → splice → savings) lives in the application layer.

use std::path::Path;

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::edit::write_dispatch::{
    DeleteInput, ExtractInput, ReplaceInput, ReplaceMode, ReplaceOutput,
};
use crate::application::facades;
use crate::output::Formatter;

use super::server::RlmServer;

/// Handle the `replace` tool: preview or apply a replacement. The
/// adapter maps the `preview` flag to [`ReplaceMode`] and dispatches
/// on the typed [`ReplaceOutput`] returned by the facade — both
/// branches use the same single application touchpoint.
pub fn handle_replace(
    project_root: &Path,
    params: &super::tools::ReplaceParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let input = ReplaceInput {
        path: &params.path,
        symbol: &params.symbol,
        parent: params.parent.as_deref(),
        code: &params.code,
    };
    let mode = if params.preview.unwrap_or(false) {
        ReplaceMode::Preview
    } else {
        ReplaceMode::Apply
    };
    let body = facades::replace_project(project_root, &input, mode).map(|out| match out {
        ReplaceOutput::Preview(diff) => RlmServer::to_json(&diff),
        ReplaceOutput::Applied(json) => json,
    });
    RlmServer::respond_string(formatter, body)
}

/// Handle the `delete` tool: remove an AST node by symbol.
pub fn handle_delete(
    project_root: &Path,
    params: &super::tools::DeleteParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let input = DeleteInput {
        path: &params.path,
        symbol: &params.symbol,
        parent: params.parent.as_deref(),
        keep_docs: params.keep_docs.unwrap_or(false),
    };
    RlmServer::respond_string(formatter, facades::delete_project(project_root, &input))
}

/// Handle the `insert` tool: insert code at a specified position.
pub fn handle_insert(
    project_root: &Path,
    params: &super::tools::InsertParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let input = crate::application::edit::write_dispatch::InsertInput {
        path: &params.path,
        position: &params.position,
        code: &params.code,
    };
    RlmServer::respond_string(formatter, facades::insert_project(project_root, &input))
}

/// Handle the `extract` tool: move symbols from one file to another.
pub fn handle_extract(
    project_root: &Path,
    params: &super::tools::ExtractParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let input = ExtractInput {
        path: &params.path,
        symbols: &params.symbols,
        to: &params.to,
        parent: params.parent.as_deref(),
    };
    RlmServer::respond_string(formatter, facades::extract_project(project_root, &input))
}
