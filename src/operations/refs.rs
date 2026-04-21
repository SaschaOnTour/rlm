//! Refs operations shared between CLI and MCP.
//!
//! Provides consistent behavior for finding all usages/call sites of a symbol.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Result of finding all references to a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct RefsResult {
    /// The symbol name.
    pub symbol: String,
    /// The list of references.
    pub refs: Vec<RefHit>,
    /// Total count of references.
    pub count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// A single reference hit.
#[derive(Debug, Clone, Serialize)]
pub struct RefHit {
    /// The kind of reference (call, import, `type_use`).
    pub kind: String,
    /// The line number.
    pub line: u32,
    /// The column number.
    pub col: u32,
    /// The chunk ID containing this reference.
    /// Note: Using `cid` for consistency (was inconsistent between CLI/MCP before).
    pub chunk_id: i64,
}

/// Find all references (usages/call sites) of a symbol.
pub fn get_refs(db: &Database, symbol: &str) -> Result<RefsResult> {
    let refs = db.get_refs_to(symbol)?;

    let hits: Vec<RefHit> = refs
        .iter()
        .map(|r| RefHit {
            kind: r.ref_kind.as_str().to_string(),
            line: r.line,
            col: r.col,
            chunk_id: r.chunk_id,
        })
        .collect();

    let count = hits.len();

    let mut result = RefsResult {
        symbol: symbol.to_string(),
        refs: hits,
        count,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
#[path = "refs_tests.rs"]
mod tests;
