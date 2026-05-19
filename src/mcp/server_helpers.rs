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

use crate::output::Formatter;

use super::server::RlmServer;

/// MCP output byte limit (~25K tokens at 2 bytes/token for JSON).
const MAX_MCP_OUTPUT_BYTES: usize = 50_000;

// -- Output helpers ----------------------------------------------------------

impl RlmServer {
    pub(crate) fn to_json<T: Serialize>(val: &T) -> String {
        crate::output::to_json(val)
    }

    pub(crate) fn success_text(formatter: Formatter, text: String) -> CallToolResult {
        CallToolResult::success(vec![Content::text(finalize(formatter, text))])
    }

    pub(crate) fn error_text(formatter: Formatter, msg: String) -> CallToolResult {
        let json = crate::output::to_json(&serde_json::json!({"error": msg}));
        CallToolResult::error(vec![Content::text(finalize(formatter, json))])
    }

    /// Map a `Result<String, E>` into the success/error `CallToolResult`
    /// shape every MCP handler emits. Callers that get a typed body
    /// (`OperationResponse`, `ReadOutput`, …) pre-extract the `.body`
    /// field with `.map(|r| r.body)` before handing off.
    pub(crate) fn respond_string<E: std::fmt::Display>(
        formatter: Formatter,
        result: Result<String, E>,
    ) -> Result<CallToolResult, McpError> {
        match result {
            Ok(s) => Ok(Self::success_text(formatter, s)),
            Err(e) => Ok(Self::error_text(formatter, e.to_string())),
        }
    }

    /// Like `respond_string`, but serialises the success payload first.
    /// Use for facades that return typed structs instead of pre-built
    /// JSON envelopes.
    pub(crate) fn respond_json<T: Serialize, E: std::fmt::Display>(
        formatter: Formatter,
        result: Result<T, E>,
    ) -> Result<CallToolResult, McpError> {
        match result {
            Ok(val) => Ok(Self::success_text(formatter, Self::to_json(&val))),
            Err(e) => Ok(Self::error_text(formatter, e.to_string())),
        }
    }
}

/// Guard the raw JSON, then apply the caller-configured formatter. Run in
/// this order so `guard_output` stays format-agnostic and any truncation
/// notice is reformatted alongside the payload.
fn finalize(formatter: Formatter, raw: String) -> String {
    let guarded = guard_output(raw);
    let cow = formatter.reformat_str(&guarded);
    if matches!(cow, std::borrow::Cow::Borrowed(_)) {
        guarded
    } else {
        cow.into_owned()
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
