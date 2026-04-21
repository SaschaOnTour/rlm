//! Callgraph building shared between CLI and MCP.

use std::collections::HashSet;

use serde::Serialize;

use crate::db::Database;
use crate::domain::chunk::RefKind;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Result of building a call graph for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct CallgraphResult {
    /// The symbol being analyzed.
    pub symbol: String,
    /// Functions/methods that call this symbol.
    pub callers: Vec<String>,
    /// Functions/methods that this symbol calls.
    pub callees: Vec<String>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Build a call graph for the given symbol.
///
/// Returns the list of callers (who calls this symbol) and callees
/// (what this symbol calls).
pub fn build_callgraph(db: &Database, symbol: &str) -> Result<CallgraphResult> {
    // Callers: single JOIN lookup replaces the per-ref get_chunk_by_id
    // loop the legacy code ran to turn each reference into its caller
    // symbol name. See `Database::get_refs_with_context`.
    let caller_refs = db.get_refs_with_context(symbol)?;
    let caller_names: Vec<String> = caller_refs
        .into_iter()
        .map(|rc| rc.containing_symbol)
        .collect();

    // Callees: chunks for this symbol, then refs from each chunk.
    let chunks = db.get_chunks_by_ident(symbol)?;
    let mut callees_refs = Vec::new();
    for chunk in &chunks {
        let refs = db.get_refs_from_chunk(chunk.id)?;
        callees_refs.extend(refs);
    }

    // Extract callee names (only Call refs, deduplicated)
    let callee_names: Vec<String> = callees_refs
        .iter()
        .filter(|r| r.ref_kind == RefKind::Call)
        .map(|r| r.target_ident.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let mut result = CallgraphResult {
        symbol: symbol.to_string(),
        callers: caller_names,
        callees: callee_names,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
#[path = "callgraph_refs_tests.rs"]
mod refs_tests;
#[cfg(test)]
#[path = "callgraph_tests.rs"]
mod tests;
