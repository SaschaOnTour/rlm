//! Helper functions and server startup for the MCP server.
//!
//! Extracted from `server.rs` for SRP compliance. Contains the
//! output-formatting helpers every `#[tool]` method uses, the
//! [`RlmSession`] factory methods the tools delegate through, and the
//! `start_mcp_server` entry point.

use std::path::PathBuf;

use rmcp::model::{CallToolResult, Content};
use rmcp::{ErrorData as McpError, ServiceExt};
use serde::Serialize;

use crate::application::session::RlmSession;
use crate::output::Formatter;

use super::server::RlmServer;

/// MCP output byte limit (~25K tokens at 2 bytes/token for JSON).
const MAX_MCP_OUTPUT_BYTES: usize = 50_000;

// -- Session factories -------------------------------------------------------

impl RlmServer {
    /// Open a session for the current project. Required for every
    /// read-side and write-side tool: the session refreshes staleness
    /// automatically so every call sees a current index. Unlike the
    /// CLI, MCP does NOT auto-index — if no index exists, the tool
    /// returns an `invalid_request` error so the client can call the
    /// `index` tool first.
    pub(crate) fn ensure_session(&self) -> Result<RlmSession, McpError> {
        match RlmSession::try_open_existing(self.project_root()) {
            Ok(Some(session)) => Ok(session),
            Ok(None) => Err(McpError::invalid_request(
                "Index not found. Call the 'index' tool first.",
                None,
            )),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Open a session only if the index already exists. Used by the
    /// `insert` tool — insert can still write to disk without an
    /// index; the response just advertises `reindexed: false`.
    pub(crate) fn try_open_session(&self) -> Option<RlmSession> {
        RlmSession::try_open_existing(self.project_root())
            .ok()
            .flatten()
    }

    pub(crate) fn to_json<T: Serialize>(val: &T) -> String {
        crate::output::to_json(val)
    }

    pub(crate) fn success_text(formatter: Formatter, text: String) -> CallToolResult {
        // Guard first (on raw JSON), then reformat. This keeps guard_output
        // format-agnostic and lets the caller-configured format apply uniformly
        // to both the payload and any truncation notice.
        let guarded = guard_output(text);
        let cow = formatter.reformat_str(&guarded);
        let formatted = if matches!(cow, std::borrow::Cow::Borrowed(_)) {
            guarded
        } else {
            cow.into_owned()
        };
        CallToolResult::success(vec![Content::text(formatted)])
    }

    pub(crate) fn error_text(formatter: Formatter, msg: String) -> CallToolResult {
        // Build raw JSON first, then guard it, then reformat. This matches
        // success_text: guard_output stays format-agnostic, while the
        // caller-configured formatter applies uniformly to the payload and
        // any truncation notice.
        let json = crate::output::to_json(&serde_json::json!({"error": msg}));
        let guarded = guard_output(json);
        let cow = formatter.reformat_str(&guarded);
        let formatted = if matches!(cow, std::borrow::Cow::Borrowed(_)) {
            guarded
        } else {
            cow.into_owned()
        };
        CallToolResult::error(vec![Content::text(formatted)])
    }
}

/// Guard against MCP output truncation by Claude Code.
///
/// CC silently truncates MCP results exceeding 25K tokens. This function
/// replaces oversized results with a truncation notice so the agent can
/// narrow its query instead of receiving silently incomplete data.
pub(crate) fn guard_output(text: String) -> String {
    if text.len() <= MAX_MCP_OUTPUT_BYTES {
        return text;
    }
    serde_json::json!({
        "truncated": true,
        "actual_bytes": text.len(),
        "limit_bytes": MAX_MCP_OUTPUT_BYTES,
        "hint": "Result exceeded 25K token MCP limit. Narrow your query with path or symbol filters."
    })
    .to_string()
}

// -- Server startup ----------------------------------------------------------

/// Start the MCP server on stdio transport.
// qual:api
pub async fn start_mcp_server() -> crate::error::Result<()> {
    // Initialize tracing to stderr (stdout is the MCP transport)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting rlm MCP server");

    // Determine project root from current working directory
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Construct the formatter from project config (MCP has no CLI flag).
    let config = crate::config::Config::new(&project_root);
    let formatter = Formatter::from_str_loose(&config.settings.output.format);

    let server = RlmServer::new(project_root, formatter);

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| crate::error::RlmError::Mcp(format!("{e}")))?;

    tracing::info!("MCP server running on stdio");

    service
        .waiting()
        .await
        .map_err(|e| crate::error::RlmError::Mcp(format!("{e}")))?;

    Ok(())
}

#[cfg(test)]
#[path = "server_helpers_tests.rs"]
mod tests;
