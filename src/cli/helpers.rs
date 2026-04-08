//! Shared helpers for CLI command handlers.
//!
//! Extracted from `handlers.rs` for SRP compliance. Contains common
//! error-mapping, config/db access, and reusable sub-operations.

use crate::cli::output;
use crate::config::Config;
use crate::db::Database;
use crate::indexer;
use crate::models::token_estimate::estimate_tokens;
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

/// Emit a read_symbol result and record savings (integration: calls only).
pub fn emit_read_symbol(db: &Database, path: &str, json: &str) {
    let out_tokens = estimate_tokens(json.len());
    let alt_tokens = savings::alternative_single_file(db, path).unwrap_or(out_tokens);
    savings::record(db, "read_symbol", out_tokens, alt_tokens, 1);
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
