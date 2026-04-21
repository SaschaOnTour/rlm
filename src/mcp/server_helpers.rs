//! Helper functions and server startup for the MCP server.
//!
//! Extracted from `server.rs` for SRP compliance. Contains the `RlmServer`
//! helper methods (config, db access, JSON formatting, file operations)
//! and the `start_mcp_server` entry point.

use std::path::PathBuf;

use rmcp::model::{CallToolResult, Content};
use rmcp::{ErrorData as McpError, ServiceExt};
use serde::Serialize;

use crate::config::Config;
use crate::db::Database;
use crate::domain::token_budget::estimate_json_tokens;
use crate::operations;
use crate::operations::savings;
use crate::output::Formatter;

use super::server::RlmServer;

/// MCP output byte limit (~25K tokens at 2 bytes/token for JSON).
const MAX_MCP_OUTPUT_BYTES: usize = 50_000;

// -- Helper functions --------------------------------------------------------

impl RlmServer {
    pub(crate) fn config(&self) -> Config {
        Config::new(self.project_root())
    }

    /// Get the database. Returns an error if the index doesn't exist.
    /// Unlike the CLI, MCP does NOT auto-index to avoid blocking on large projects.
    ///
    /// Runs the staleness check so every tool call sees an up-to-date index
    /// (picks up CC-native Edit/Write, external edits, `git pull`, ...). Set
    /// `RLM_SKIP_REFRESH=1` in the MCP server env to skip.
    ///
    /// Uses `Database::open_required` to distinguish "index truly missing"
    /// (→ `invalid_request`) from real I/O / permission errors (→
    /// `internal_error`), rather than collapsing both into "not found".
    pub(crate) fn ensure_db(&self) -> Result<Database, McpError> {
        let config = self.config();
        let db = Database::open_required(&config.db_path).map_err(|e| match e {
            crate::error::RlmError::IndexNotFound => {
                McpError::invalid_request("Index not found. Call the 'index' tool first.", None)
            }
            other => McpError::internal_error(other.to_string(), None),
        })?;
        crate::application::index::staleness::ensure_index_fresh(&db, &config)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(db)
    }

    /// Try to open the database without requiring the index to exist.
    /// Returns `None` if the index hasn't been created yet.
    pub(crate) fn try_open_db(&self) -> Option<Database> {
        Database::open_if_exists(&self.config().db_path)
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

// -- Helper method for read with metadata enrichment -------------------------

impl RlmServer {
    /// Serialize a value, record token savings, and return a success result (operation: calls only).
    fn serialize_and_record<T: Serialize>(
        db: &Database,
        path: &str,
        val: &T,
        formatter: Formatter,
    ) -> Result<CallToolResult, McpError> {
        let json = Self::to_json(val);
        let out_tokens = estimate_json_tokens(json.len());
        savings::record_read_symbol(db, out_tokens, path);
        Ok(Self::success_text(formatter, json))
    }

    /// Build the read-symbol response, optionally enriching with metadata (integration: calls only).
    // qual:allow(iosp) reason: "metadata enrichment dispatch cannot be further separated"
    pub(crate) fn read_symbol_result<T: Serialize>(
        db: &Database,
        params: &super::tools::ReadParams,
        chunks: &T,
        formatter: Formatter,
    ) -> Result<CallToolResult, McpError> {
        let include_metadata = params.metadata.unwrap_or(false);

        if include_metadata {
            if let Some(sym) = &params.symbol {
                let type_info = operations::get_type_info(db, sym).ok();
                let signature = operations::get_signature(db, sym).ok();

                #[derive(Serialize)]
                struct Enriched<'a, T: Serialize> {
                    chunks: &'a T,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    type_info: Option<operations::TypeInfoResult>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    signature: Option<operations::SignatureResult>,
                }

                let enriched = Enriched {
                    chunks,
                    type_info,
                    signature,
                };
                return Self::serialize_and_record(db, &params.path, &enriched, formatter);
            }
        }

        Self::serialize_and_record(db, &params.path, chunks, formatter)
    }
}

/// Guard against MCP output truncation by Claude Code.
///
/// CC silently truncates MCP results exceeding 25K tokens. This function
/// replaces oversized results with a truncation notice so the agent can
/// narrow its query instead of receiving silently incomplete data.
///
/// **Known limitation:** Some savings recording functions (`record_file_op`,
/// `record_scoped_op`, `record_symbol_op`) estimate tokens from the pre-guard
/// JSON. If the guard truncates, recorded savings for that operation are slightly
/// overstated. This only affects responses >50K bytes (rare) and has negligible
/// impact on aggregate reports.
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
