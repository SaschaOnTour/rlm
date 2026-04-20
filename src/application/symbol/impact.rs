//! Impact analysis shared between CLI and MCP.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// A single location that would be impacted by changing a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactEntry {
    /// File path containing the reference.
    pub file: String,
    /// Symbol containing the reference.
    pub in_symbol: String,
    /// Line number of the reference.
    pub line: u32,
    /// Kind of reference (call, import, `type_use`).
    pub ref_kind: String,
}

/// Result of impact analysis for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactResult {
    /// The symbol being analyzed.
    pub symbol: String,
    /// List of impacted locations.
    pub impacted: Vec<ImpactEntry>,
    /// Total count of impacted locations.
    pub count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

impl ImpactResult {
    /// Number of distinct files containing at least one impacted location.
    ///
    /// This is the count the savings middleware's `SymbolFiles` cost
    /// model needs for `files_touched` — using `count` would overstate
    /// `alt_calls` whenever multiple hits share a file.
    #[must_use]
    pub fn file_count(&self) -> u64 {
        use std::collections::HashSet;
        self.impacted
            .iter()
            .map(|e| e.file.as_str())
            .collect::<HashSet<_>>()
            .len() as u64
    }
}

/// Analyze the impact of changing a symbol.
///
/// Returns all locations (file, containing symbol, line, ref kind)
/// that reference this symbol and would need updating if it changes.
pub fn analyze_impact(db: &Database, symbol: &str) -> Result<ImpactResult> {
    // Single JOIN query instead of the legacy N+1 (get_chunk_by_id +
    // get_all_files per ref). See `Database::get_refs_with_context`.
    let refs_with_ctx = db.get_refs_with_context(symbol)?;

    let impacted: Vec<ImpactEntry> = refs_with_ctx
        .into_iter()
        .map(|rc| ImpactEntry {
            file: rc.file_path,
            in_symbol: rc.containing_symbol,
            line: rc.reference.line,
            ref_kind: rc.reference.ref_kind.as_str().to_string(),
        })
        .collect();

    let count = impacted.len();
    let mut result = ImpactResult {
        symbol: symbol.to_string(),
        impacted,
        count,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
#[path = "impact_ref_kind_tests.rs"]
mod ref_kind_tests;
#[cfg(test)]
#[path = "impact_tests.rs"]
mod tests;
