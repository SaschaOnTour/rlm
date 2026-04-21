//! Signature operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting symbol signatures and call site counts.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Result of getting a symbol's signature.
#[derive(Debug, Clone, Serialize)]
pub struct SignatureResult {
    /// The symbol name.
    pub symbol: String,
    /// The signatures (may have multiple if symbol is defined in multiple places).
    pub signatures: Vec<String>,
    /// The count of all call sites.
    pub ref_count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Get the signature of a symbol plus the count of all call sites.
pub fn get_signature(db: &Database, symbol: &str) -> Result<SignatureResult> {
    let chunks = db.get_chunks_by_ident(symbol)?;
    let refs = db.get_refs_to(symbol)?;

    let sigs: Vec<String> = chunks.iter().filter_map(|c| c.signature.clone()).collect();

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
