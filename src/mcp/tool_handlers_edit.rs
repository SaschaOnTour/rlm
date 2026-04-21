//! MCP write-side tool handlers: `replace`, `delete`, `insert`, `extract`.
//!
//! Every handler parses the rmcp [`Parameters`] into an
//! application-layer input struct, calls one [`RlmSession`] method,
//! and emits the result via [`RlmServer`]. All orchestration
//! (op → reindex → splice → savings) lives in the application layer.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::edit::inserter::InsertPosition;
use crate::application::edit::write_dispatch::{DeleteInput, ExtractInput, ReplaceInput};
use crate::application::session::RlmSession;
use crate::output::Formatter;

use super::server::RlmServer;

/// Handle the `replace` tool: preview or apply a replacement.
// qual:api
pub fn handle_replace(
    session: &RlmSession,
    params: &super::tools::ReplaceParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let input = ReplaceInput {
        path: &params.path,
        symbol: &params.symbol,
        parent: params.parent.as_deref(),
        code: &params.code,
    };
    if params.preview.unwrap_or(false) {
        return match session.replace_preview(&input) {
            Ok(diff) => Ok(RlmServer::success_text(
                formatter,
                RlmServer::to_json(&diff),
            )),
            Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
        };
    }
    match session.replace_apply(&input) {
        Ok(json) => Ok(RlmServer::success_text(formatter, json)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `delete` tool: remove an AST node by symbol.
// qual:api
pub fn handle_delete(
    session: &RlmSession,
    params: &super::tools::DeleteParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let input = DeleteInput {
        path: &params.path,
        symbol: &params.symbol,
        parent: params.parent.as_deref(),
        keep_docs: params.keep_docs.unwrap_or(false),
    };
    match session.delete(&input) {
        Ok(json) => Ok(RlmServer::success_text(formatter, json)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Grouped inputs to `handle_insert` so the signature stays below
/// the SRP parameter ceiling. Mirrors
/// [`crate::application::edit::write_dispatch::InsertInput`] — kept
/// on the MCP side because `insert` is the one write tool that must
/// still work without an index.
pub struct InsertHandlerInput<'a> {
    pub path: &'a str,
    pub position: &'a InsertPosition,
    pub code: &'a str,
}

/// Backwards-compatible alias for the type previously re-exported by
/// `tool_handlers::InsertInput`.
pub type InsertInput<'a> = InsertHandlerInput<'a>;

/// Handle the `insert` tool: insert code at a specified position.
///
/// Takes the optional session directly so we can succeed with
/// `reindexed: false` when no index exists.
// qual:api
pub fn handle_insert(
    session: Option<&RlmSession>,
    input: &InsertHandlerInput<'_>,
    project_root: &std::path::Path,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let dispatch_input = crate::application::edit::write_dispatch::InsertInput {
        path: input.path,
        position: input.position,
        code: input.code,
    };
    let result = match session {
        Some(s) => s.insert(&dispatch_input),
        None => RlmSession::insert_without_index(project_root, &dispatch_input),
    };
    match result {
        Ok(json) => Ok(RlmServer::success_text(formatter, json)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `extract` tool: move symbols from one file to another.
// qual:api
pub fn handle_extract(
    session: &RlmSession,
    params: &super::tools::ExtractParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let input = ExtractInput {
        path: &params.path,
        symbols: &params.symbols,
        to: &params.to,
        parent: params.parent.as_deref(),
    };
    match session.extract(&input) {
        Ok(json) => Ok(RlmServer::success_text(formatter, json)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}
