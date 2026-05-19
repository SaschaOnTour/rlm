//! Callgraph building shared between CLI and MCP.

use std::collections::HashSet;

use serde::Serialize;

use crate::db::Database;
use crate::domain::chunk::RefKind;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// One symbol referenced from a call graph — `(ident, parent)` pair.
/// Used for both callers (who calls this symbol) and callees (what
/// this symbol calls). Polysemic callees emit one entry per distinct
/// parent so the consumer sees the full candidate set instead of a
/// bare ident.
#[derive(Debug, Clone, Serialize, Eq, PartialEq, Hash)]
pub struct SymbolRef {
    /// Symbol identifier.
    pub ident: String,
    /// `Some(Type)` for methods on a concrete type, `None` for free
    /// functions or external (un-indexed) targets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// Result of building a call graph for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct CallgraphResult {
    /// The symbol being analyzed.
    pub symbol: String,
    /// Functions/methods that call this symbol.
    pub callers: Vec<SymbolRef>,
    /// Functions/methods that this symbol calls. Polysemic callees
    /// (e.g. `new` defined on multiple types) emit one entry per
    /// candidate parent so the static-call-graph ambiguity is
    /// surfaced rather than collapsed into a bare ident.
    pub callees: Vec<SymbolRef>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Build a call graph for the given symbol.
///
/// Returns the list of callers (who calls this symbol) and callees
/// (what this symbol calls), each tagged with `parent` so polysemic
/// idents are disambiguated at every output boundary.
pub fn build_callgraph(db: &Database, symbol: &str) -> Result<CallgraphResult> {
    // Callers: single JOIN lookup gives both the containing symbol
    // and its parent (for methods).
    let caller_refs = db.get_refs_with_context(symbol)?;
    let callers: Vec<SymbolRef> = caller_refs
        .into_iter()
        .map(|rc| SymbolRef {
            ident: rc.containing_symbol,
            parent: rc.containing_parent,
        })
        .collect();

    let chunks = db.get_chunks_by_ident(symbol)?;
    let callees = collect_callees_with_parents(db, &chunks)?;

    let mut result = CallgraphResult {
        symbol: symbol.to_string(),
        callers,
        callees,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Walk every outgoing call from the given chunks and resolve each
/// distinct target ident into [`SymbolRef`]s tagged with parent.
///
/// Polysemic targets emit one entry per candidate parent (so the
/// static-call-graph ambiguity is surfaced rather than collapsed).
/// External (un-indexed) targets get a single entry with
/// `parent = None`. Shared between [`build_callgraph`] and
/// [`super::context::build_context`].
pub(super) fn collect_callees_with_parents(
    db: &Database,
    chunks: &[crate::domain::chunk::Chunk],
) -> Result<Vec<SymbolRef>> {
    let callee_idents = collect_callee_idents(db, chunks)?;
    if callee_idents.is_empty() {
        return Ok(Vec::new());
    }
    let ident_refs: Vec<&str> = callee_idents.iter().map(String::as_str).collect();
    let target_chunks = db.get_chunks_by_idents(&ident_refs)?;
    let mut by_ident: std::collections::HashMap<&str, Vec<Option<String>>> =
        std::collections::HashMap::new();
    for c in &target_chunks {
        by_ident.entry(&c.ident).or_default().push(c.parent.clone());
    }
    let mut callees: HashSet<SymbolRef> = HashSet::new();
    for ident in &callee_idents {
        match by_ident.get(ident.as_str()) {
            None => {
                callees.insert(SymbolRef {
                    ident: ident.clone(),
                    parent: None,
                });
            }
            Some(parents) => {
                for parent in parents {
                    callees.insert(SymbolRef {
                        ident: ident.clone(),
                        parent: parent.clone(),
                    });
                }
            }
        }
    }
    Ok(callees.into_iter().collect())
}

/// Distinct call-target idents (Call ref kind only) reachable
/// from any of the given chunks.
fn collect_callee_idents(
    db: &Database,
    chunks: &[crate::domain::chunk::Chunk],
) -> Result<HashSet<String>> {
    let mut callee_refs = Vec::new();
    for chunk in chunks {
        let refs = db.get_refs_from_chunk(chunk.id)?;
        callee_refs.extend(refs);
    }
    Ok(callee_refs
        .iter()
        .filter(|r| r.ref_kind == RefKind::Call)
        .map(|r| r.target_ident.clone())
        .collect())
}

#[cfg(test)]
#[path = "callgraph_refs_tests.rs"]
mod refs_tests;
#[cfg(test)]
#[path = "callgraph_tests.rs"]
mod tests;
