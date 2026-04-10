//! Shared helpers for CLI command handlers.
//!
//! Extracted from `handlers.rs` for SRP compliance. Contains common
//! error-mapping, config/db access, and reusable sub-operations.

use crate::cli::output;
use crate::config::Config;
use crate::db::Database;
use crate::indexer;
use crate::models::token_estimate::estimate_json_tokens;
use crate::operations::savings;

pub type CmdResult = Result<(), Box<dyn std::fmt::Display>>;

pub fn print_json(json: &str) {
    println!("{json}");
}

pub fn map_err(e: impl std::fmt::Display + 'static) -> Box<dyn std::fmt::Display> {
    Box::new(e.to_string())
}

pub fn get_config() -> Result<Config, Box<dyn std::fmt::Display>> {
    Config::from_cwd().map_err(map_err)
}

pub fn get_db(config: &Config) -> Result<Database, Box<dyn std::fmt::Display>> {
    indexer::ensure_index(config).map_err(map_err)
}

/// Format chunks as JSON, optionally including metadata (integration: calls only).
pub fn format_chunks_json(
    db: &Database,
    sym: &str,
    chunks: &serde_json::Value,
    metadata: bool,
) -> String {
    if metadata {
        let type_info = crate::operations::get_type_info(db, sym).ok();
        let signature = crate::operations::get_signature(db, sym).ok();
        output::format_json(&serde_json::json!({
            "chunks": chunks,
            "type_info": type_info,
            "signature": signature,
        }))
    } else {
        output::format_json(chunks)
    }
}

/// Build and print a write result with reindex status, matching MCP output format.
///
/// Returns the result JSON string so callers can use its length for savings.
pub fn print_write_result(
    db: &Database,
    config: &Config,
    rel_path: &str,
    source: indexer::PreviewSource<'_>,
) -> String {
    let json = indexer::reindex_with_result(db, config, rel_path, source);
    print_json(&json);
    json
}

/// Emit a read_symbol result and record savings (integration: calls only).
pub fn emit_read_symbol(db: &Database, path: &str, json: &str) {
    let out_tokens = estimate_json_tokens(json.len());
    savings::record_read_symbol(db, out_tokens, path);
    print_json(json);
}

/// Generic handler for commands that operate on a single file with savings recording.
pub fn cmd_single_file_op<T: serde::Serialize>(
    command: &str,
    path: &str,
    op: impl FnOnce(&Database, &str) -> crate::error::Result<T>,
) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = op(&db, path).map_err(map_err)?;
    let json = savings::record_file_op(&db, command, &result, path);
    print_json(&json);
    Ok(())
}

/// Parse a partition strategy string into a `Strategy` enum.
pub fn parse_strategy(
    s: &str,
) -> Result<crate::rlm::partition::Strategy, Box<dyn std::fmt::Display>> {
    if s == "semantic" {
        Ok(crate::rlm::partition::Strategy::Semantic)
    } else if let Some(rest) = s.strip_prefix("uniform:") {
        let n: usize = rest.parse().map_err(map_err)?;
        if n == 0 {
            return Err(map_err("uniform chunk size must be >= 1"));
        }
        Ok(crate::rlm::partition::Strategy::Uniform(n))
    } else if let Some(rest) = s.strip_prefix("keyword:") {
        Ok(crate::rlm::partition::Strategy::Keyword(rest.to_string()))
    } else {
        Err(map_err(
            "strategy must be: semantic, uniform:N, or keyword:PATTERN",
        ))
    }
}

/// Determine whether unknown-only filtering should be applied (operation: logic only).
pub fn should_filter_unknown(unknown_only: bool, all: bool) -> bool {
    unknown_only || !all
}
