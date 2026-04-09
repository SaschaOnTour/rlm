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
use crate::models::token_estimate::estimate_tokens;
use crate::operations;
use crate::operations::savings;

use super::server::RlmServer;

// -- Helper functions --------------------------------------------------------

impl RlmServer {
    pub(crate) fn config(&self) -> Config {
        Config::new(self.project_root())
    }

    /// Get the database. Returns an error if the index doesn't exist.
    /// Unlike the CLI, MCP does NOT auto-index to avoid blocking on large projects.
    pub(crate) fn ensure_db(&self) -> Result<Database, McpError> {
        Database::open_if_exists(&self.config().db_path).ok_or_else(|| {
            McpError::invalid_request("Index not found. Call the 'index' tool first.", None)
        })
    }

    /// Try to open the database without requiring the index to exist.
    /// Returns `None` if the index hasn't been created yet.
    pub(crate) fn try_open_db(&self) -> Option<Database> {
        Database::open_if_exists(&self.config().db_path)
    }

    pub(crate) fn to_json<T: Serialize>(val: &T) -> String {
        serde_json::to_string(val).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
    }

    pub(crate) fn success_text(text: String) -> CallToolResult {
        CallToolResult::success(vec![Content::text(text)])
    }

    pub(crate) fn error_text(msg: String) -> CallToolResult {
        CallToolResult::error(vec![Content::text(
            serde_json::json!({"error": msg}).to_string(),
        )])
    }
}

// -- Helper method for read with metadata enrichment -------------------------

impl RlmServer {
    /// Serialize a value, record token savings, and return a success result (operation: calls only).
    fn serialize_and_record<T: Serialize>(
        db: &Database,
        path: &str,
        val: &T,
    ) -> Result<CallToolResult, McpError> {
        let json = Self::to_json(val);
        let out_tokens = estimate_tokens(json.len());
        savings::record_read_symbol(db, out_tokens, path);
        Ok(Self::success_text(json))
    }

    /// Build the read-symbol response, optionally enriching with metadata (integration: calls only).
    // qual:allow(iosp) reason: "metadata enrichment dispatch cannot be further separated"
    pub(crate) fn read_symbol_result<T: Serialize>(
        db: &Database,
        params: &super::tools::ReadParams,
        chunks: &T,
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
                return Self::serialize_and_record(db, &params.path, &enriched);
            }
        }

        Self::serialize_and_record(db, &params.path, chunks)
    }
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

    let server = RlmServer::new(project_root);

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| crate::error::RlmError::Other(format!("MCP server error: {e}")))?;

    tracing::info!("MCP server running on stdio");

    service
        .waiting()
        .await
        .map_err(|e| crate::error::RlmError::Other(format!("MCP server error: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text_sets_is_error_true() {
        let result = RlmServer::error_text("something failed".into());
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn success_text_does_not_set_is_error() {
        let result = RlmServer::success_text("ok".into());
        assert_ne!(result.is_error, Some(true));
    }

    #[test]
    fn error_text_contains_message() {
        let result = RlmServer::error_text("disk full".into());
        let text = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .map(|t| t.text.clone())
            .unwrap_or_default();
        assert!(text.contains("disk full"));
    }
}
