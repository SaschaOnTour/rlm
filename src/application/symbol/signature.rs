//! Signature operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting symbol signatures and call site counts.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// One concrete signature for the queried symbol, tagged with its
/// parent type. Polysemic idents (e.g. `new` defined on every type
/// in the codebase) need this to disambiguate which signature
/// belongs to which `impl Type` block.
#[derive(Debug, Clone, Serialize)]
pub struct SignatureEntry {
    /// `Some(Type)` for `impl Type { fn ident }`, `None` for free
    /// functions / module-level items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The signature text, exactly as the parser captured it.
    pub signature: String,
}

/// Result of getting a symbol's signature.
#[derive(Debug, Clone, Serialize)]
pub struct SignatureResult {
    /// The symbol name.
    pub symbol: String,
    /// The signatures (may have multiple if symbol is defined in
    /// multiple places). Each entry tags its parent so polysemy is
    /// machine-readable.
    pub signatures: Vec<SignatureEntry>,
    /// The count of all call sites.
    pub ref_count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Get the signature of a symbol plus the count of all call sites.
///
/// When `parent` is `Some(...)`, only signatures of methods on that
/// parent type are surfaced — keeps the metadata aligned with the
/// caller's polysemy filter (`rlm read --symbol new --parent Foo
/// --metadata` doesn't bleed `Bar::new` into the signatures list).
/// `None` returns every signature across the polysemic group.
pub fn get_signature(db: &Database, symbol: &str, parent: Option<&str>) -> Result<SignatureResult> {
    let chunks = db.get_chunks_by_ident(symbol)?;
    let refs = db.get_refs_to(symbol)?;

    let sigs: Vec<SignatureEntry> = chunks
        .iter()
        .filter(|c| match parent {
            None => true,
            Some(p) => c.parent.as_deref() == Some(p),
        })
        .filter_map(|c| {
            c.signature.as_ref().map(|s| SignatureEntry {
                parent: c.parent.clone(),
                signature: s.clone(),
            })
        })
        .collect();

    let mut result = SignatureResult {
        symbol: symbol.to_string(),
        signatures: sigs,
        ref_count: refs.len(),
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
#[path = "signature_tests.rs"]
mod tests;
