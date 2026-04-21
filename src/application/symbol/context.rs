//! Context building shared between CLI and MCP.

use std::collections::HashSet;

use serde::Serialize;

use crate::db::Database;
use crate::domain::chunk::RefKind;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

use super::callgraph::{build_callgraph, CallgraphResult};
use super::SymbolQuery;

/// Complete context information for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ContextResult {
    /// The symbol being analyzed.
    pub symbol: String,
    /// Full body content of each definition.
    pub body: Vec<String>,
    /// Signatures of each definition.
    pub signatures: Vec<String>,
    /// Number of callers.
    pub caller_count: usize,
    /// Names of callees.
    pub callee_names: Vec<String>,
    /// Number of distinct files containing this symbol.
    pub file_count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Build complete context for understanding a symbol.
///
/// Returns the symbol's body content, signatures, caller count,
/// and the names of functions/methods it calls.
pub fn build_context(db: &Database, symbol: &str) -> Result<ContextResult> {
    // Get the symbol's own content
    let chunks = db.get_chunks_by_ident(symbol)?;
    let callers_refs = db.get_refs_to(symbol)?;

    // Get callees
    let mut callees = Vec::new();
    for chunk in &chunks {
        let refs = db.get_refs_from_chunk(chunk.id)?;
        callees.extend(refs);
    }

    let file_count = chunks
        .iter()
        .map(|c| c.file_id)
        .collect::<HashSet<_>>()
        .len();
    let bodies: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let sigs: Vec<String> = chunks.iter().filter_map(|c| c.signature.clone()).collect();
    let callee_names: Vec<String> = callees
        .iter()
        .filter(|r| r.ref_kind == RefKind::Call)
        .map(|r| r.target_ident.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let mut result = ContextResult {
        symbol: symbol.to_string(),
        body: bodies,
        signatures: sigs,
        caller_count: callers_refs.len(),
        callee_names,
        file_count,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Bare context query — symbol body + caller count + callee names, no
/// full callgraph expansion.
pub struct ContextQuery;

impl SymbolQuery for ContextQuery {
    type Output = ContextResult;
    const COMMAND: &'static str = "context";

    fn execute(db: &Database, symbol: &str) -> Result<Self::Output> {
        build_context(db, symbol)
    }

    fn file_count(output: &Self::Output) -> u64 {
        output.file_count as u64
    }
}

/// Combined envelope returned by [`ContextWithGraphQuery`]: the bare
/// context plus the full callgraph.
#[derive(Debug, Clone, Serialize)]
pub struct ContextWithGraph {
    pub context: ContextResult,
    pub callgraph: CallgraphResult,
}

/// Context query with full callgraph expansion.
pub struct ContextWithGraphQuery;

impl SymbolQuery for ContextWithGraphQuery {
    type Output = ContextWithGraph;
    const COMMAND: &'static str = "context";

    fn execute(db: &Database, symbol: &str) -> Result<Self::Output> {
        let context = build_context(db, symbol)?;
        let callgraph = build_callgraph(db, symbol)?;
        Ok(ContextWithGraph { context, callgraph })
    }

    fn file_count(output: &Self::Output) -> u64 {
        output.context.file_count as u64
    }
}

#[cfg(test)]
#[path = "context_graph_tests.rs"]
mod graph_tests;
#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
