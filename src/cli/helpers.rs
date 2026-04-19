//! Shared helpers for CLI command handlers.
//!
//! Extracted from `handlers.rs` for SRP compliance. Contains common
//! error-mapping, config/db access, and reusable sub-operations.

use crate::application::index as indexer;
use crate::config::Config;
use crate::db::Database;
use crate::domain::token_budget::estimate_json_tokens;
use crate::operations::savings;
use crate::output::{self, Formatter};

pub type CmdResult = Result<(), Box<dyn std::fmt::Display>>;

pub fn print_str(formatter: Formatter, s: &str) {
    formatter.print_str(s);
}

pub fn map_err(e: impl std::fmt::Display + 'static) -> Box<dyn std::fmt::Display> {
    Box::new(e.to_string())
}

pub fn get_config() -> Result<Config, Box<dyn std::fmt::Display>> {
    Config::from_cwd().map_err(map_err)
}

pub fn get_db(config: &Config) -> Result<Database, Box<dyn std::fmt::Display>> {
    let db = indexer::ensure_index(config).map_err(map_err)?;
    // Self-healing: pick up external edits (CC-native, vim, git pull, ...)
    // before the caller uses the index. Set RLM_SKIP_REFRESH=1 to skip.
    indexer::staleness::ensure_index_fresh(&db, config).map_err(map_err)?;
    Ok(db)
}

/// Serialize chunks as JSON, optionally including metadata.
///
/// Returns JSON (not TOON/Pretty) because the result is used for savings token estimation.
/// The output format is applied later via `print_str`.
pub fn format_chunks(
    db: &Database,
    sym: &str,
    chunks: &serde_json::Value,
    metadata: bool,
) -> String {
    if metadata {
        let type_info = crate::operations::get_type_info(db, sym).ok();
        let signature = crate::operations::get_signature(db, sym).ok();
        output::to_json(&serde_json::json!({
            "chunks": chunks,
            "type_info": type_info,
            "signature": signature,
        }))
    } else {
        output::to_json(chunks)
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
    formatter: Formatter,
) -> String {
    let json = indexer::reindex_with_result(db, config, rel_path, source);
    print_str(formatter, &json);
    json
}

/// Emit a read_symbol result and record savings (integration: calls only).
pub fn emit_read_symbol(db: &Database, path: &str, json: &str, formatter: Formatter) {
    let out_tokens = estimate_json_tokens(json.len());
    savings::record_read_symbol(db, out_tokens, path);
    print_str(formatter, json);
}

/// Parse a partition strategy string into a `Strategy` enum.
pub fn parse_strategy(
    s: &str,
) -> Result<crate::application::content::partition::Strategy, Box<dyn std::fmt::Display>> {
    if s == "semantic" {
        Ok(crate::application::content::partition::Strategy::Semantic)
    } else if let Some(rest) = s.strip_prefix("uniform:") {
        let n: usize = rest.parse().map_err(map_err)?;
        if n == 0 {
            return Err(map_err("uniform chunk size must be >= 1"));
        }
        Ok(crate::application::content::partition::Strategy::Uniform(n))
    } else if let Some(rest) = s.strip_prefix("keyword:") {
        Ok(crate::application::content::partition::Strategy::Keyword(
            rest.to_string(),
        ))
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
