//! MCP `read` tool handler: symbol / section retrieval.
//!
//! The business logic (chunk-by-ident, file/parent filtering,
//! metadata enrichment, section headings listing) lives in
//! [`crate::application::query::read`]. This handler only translates
//! the MCP request shape and maps typed results to a response.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::query::read::ReadSymbolInput;
use crate::application::session::RlmSession;
use crate::output::Formatter;

use super::server::RlmServer;
use super::tools::ReadParams;

/// Handle the `read` tool: read a specific symbol or markdown section.
// qual:api
pub fn handle_read(
    session: &RlmSession,
    params: &ReadParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match (&params.symbol, &params.section) {
        (Some(sym), _) => handle_read_symbol(session, params, sym, formatter),
        (_, Some(heading)) => handle_read_section(session, &params.path, heading, formatter),
        _ => Ok(RlmServer::error_text(
            formatter,
            "read requires 'symbol' or 'section'. Use Claude Code's Read for full files or line ranges.".into(),
        )),
    }
}

fn handle_read_symbol(
    session: &RlmSession,
    params: &ReadParams,
    sym: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let input = ReadSymbolInput {
        path: &params.path,
        symbol: sym,
        parent: params.parent.as_deref(),
        metadata: params.metadata.unwrap_or(false),
    };
    match session.read_symbol(&input) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

fn handle_read_section(
    session: &RlmSession,
    path: &str,
    heading: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match session.read_section(path, heading) {
        Ok(result) => match result.into_body_or_error() {
            Ok(body) => Ok(RlmServer::success_text(formatter, body)),
            Err(msg) => Ok(RlmServer::error_text(formatter, msg)),
        },
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}
