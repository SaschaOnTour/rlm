//! MCP `read` tool handler: symbol / section retrieval.
//!
//! The business logic (chunk-by-ident, file/parent filtering,
//! metadata enrichment, section headings listing) lives in
//! [`crate::application::query::read`]. This handler only translates
//! the MCP request shape and maps typed results to a response.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::query::read::{ReadSectionResult, ReadSymbolInput, MAX_SECTION_HINT};
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
        Ok(ReadSectionResult::Found { body, .. }) => Ok(RlmServer::success_text(formatter, body)),
        Ok(ReadSectionResult::NotFound {
            heading,
            available,
            total,
        }) => Ok(RlmServer::error_text(
            formatter,
            section_not_found_hint(&heading, &available, total),
        )),
        Ok(ReadSectionResult::FileNotFound { path }) => Ok(RlmServer::error_text(
            formatter,
            format!(
                "File not found: {path}. Run 'index' to update, or check 'files' for available paths."
            ),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

fn section_not_found_hint(heading: &str, available: &[String], total: usize) -> String {
    if available.is_empty() {
        return format!("section not found: {heading}. File has no sections.");
    }
    if total > available.len() {
        format!(
            "section not found: {heading}. Available ({total} total, first {MAX_SECTION_HINT}): {}",
            available.join(", ")
        )
    } else {
        format!(
            "section not found: {heading}. Available: {}",
            available.join(", ")
        )
    }
}
