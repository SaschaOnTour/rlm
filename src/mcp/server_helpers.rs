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
        crate::indexer::staleness::ensure_index_fresh(&db, &config)
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

    pub(crate) fn success_text(text: String) -> CallToolResult {
        // Guard first (on raw JSON), then reformat. This keeps guard_output
        // format-agnostic and lets the caller-configured format apply uniformly
        // to both the payload and any truncation notice.
        let guarded = guard_output(text);
        let cow = crate::output::reformat_str(&guarded);
        let formatted = if matches!(cow, std::borrow::Cow::Borrowed(_)) {
            guarded
        } else {
            cow.into_owned()
        };
        CallToolResult::success(vec![Content::text(formatted)])
    }

    pub(crate) fn error_text(msg: String) -> CallToolResult {
        // Build raw JSON first, then guard it, then reformat. This matches
        // success_text: guard_output stays format-agnostic, while the
        // caller-configured formatter applies uniformly to the payload and
        // any truncation notice.
        let json = crate::output::to_json(&serde_json::json!({"error": msg}));
        let guarded = guard_output(json);
        let cow = crate::output::reformat_str(&guarded);
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
    ) -> Result<CallToolResult, McpError> {
        let json = Self::to_json(val);
        let out_tokens = estimate_json_tokens(json.len());
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

    // Initialize output format from config (MCP has no CLI flag)
    let config = crate::config::Config::new(&project_root);
    crate::output::init(crate::output::OutputFormat::from_str_loose(
        &config.settings.output.format,
    ));

    let server = RlmServer::new(project_root);

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

    #[test]
    fn guard_output_passes_small_result() {
        let small = "{\"ok\":true}".to_string();
        let result = guard_output(small.clone());
        assert_eq!(result, small);
    }

    #[test]
    fn guard_output_truncates_large_result() {
        let large = "x".repeat(MAX_MCP_OUTPUT_BYTES + 1);
        let result = guard_output(large);
        assert!(result.contains("\"truncated\":true"));
        assert!(result.len() < MAX_MCP_OUTPUT_BYTES);
    }

    #[test]
    fn ensure_db_runs_staleness_check_on_mcp_path() {
        // Regression test: the MCP canonical DB-open (RlmServer::ensure_db) must
        // invoke the self-healing staleness check, mirroring the CLI `get_db`.
        // This guards against accidentally losing the wiring from P07-05.
        use std::fs;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn original() {}").unwrap();

        // Index once so the DB exists.
        let config = Config::new(tmp.path());
        crate::indexer::run_index(&config, None).unwrap();

        // Add a new symbol externally (not via rlm) — index now stale.
        fs::write(tmp.path().join("new.rs"), "fn externally_added() {}").unwrap();

        // MCP path: ensure_db should reconcile before returning the DB.
        let server = RlmServer::new(tmp.path().to_path_buf());
        let db = server.ensure_db().expect("ensure_db succeeds");

        let new_symbol_file = db.get_file_by_path("new.rs").unwrap();
        assert!(
            new_symbol_file.is_some(),
            "MCP ensure_db must pick up externally-added files"
        );
    }

    #[test]
    fn guard_output_boundary() {
        let exact = "x".repeat(MAX_MCP_OUTPUT_BYTES);
        let result = guard_output(exact.clone());
        assert_eq!(result, exact);
    }
}
