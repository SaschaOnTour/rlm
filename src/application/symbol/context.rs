//! Context building shared between CLI and MCP.

use std::collections::HashSet;

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

use super::callgraph::{build_callgraph, collect_callees_with_parents, CallgraphResult, SymbolRef};
use super::SymbolQuery;

/// One concrete definition of the queried symbol — bundles the
/// parent (when the symbol is a method), signature, and body so
/// polysemic queries (`context new`) return a structured per-impl
/// view instead of three parallel arrays the caller has to align
/// by index.
#[derive(Debug, Clone, Serialize)]
pub struct DefinitionEntry {
    /// `Some(Type)` for `impl Type { fn ident }`, `None` for free
    /// functions / module-level items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Signature text, when the parser captured one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Full body content of this definition.
    pub body: String,
}

/// Complete context information for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ContextResult {
    /// The symbol being analyzed.
    pub symbol: String,
    /// One entry per definition (parent + signature + body together).
    /// Polysemic queries return multiple entries.
    pub definitions: Vec<DefinitionEntry>,
    /// Number of callers.
    pub caller_count: usize,
    /// Functions/methods this symbol calls. Polysemic callees emit
    /// one entry per candidate parent — same shape as
    /// [`CallgraphResult::callees`].
    pub callees: Vec<SymbolRef>,
    /// Number of distinct files containing this symbol.
    pub file_count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Build complete context for understanding a symbol.
///
/// Returns the symbol's body content, signatures, caller count,
/// and the functions/methods it calls — every per-symbol slot
/// tagged with its parent so polysemy is visible end-to-end.
pub fn build_context(db: &Database, symbol: &str) -> Result<ContextResult> {
    let chunks = db.get_chunks_by_ident(symbol)?;
    let callers_refs = db.get_refs_to(symbol)?;
    let callees = collect_callees_with_parents(db, &chunks)?;

    let file_count = chunks
        .iter()
        .map(|c| c.file_id)
        .collect::<HashSet<_>>()
        .len();
    let definitions: Vec<DefinitionEntry> = chunks
        .iter()
        .map(|c| DefinitionEntry {
            parent: c.parent.clone(),
            signature: c.signature.clone(),
            body: c.content.clone(),
        })
        .collect();

    let mut result = ContextResult {
        symbol: symbol.to_string(),
        definitions,
        caller_count: callers_refs.len(),
        callees,
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
