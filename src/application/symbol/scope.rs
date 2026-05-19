//! Scope operations shared between CLI and MCP.
//!
//! Provides consistent behavior for determining what symbols are visible at a location.

use serde::Serialize;

use crate::application::FileQuery;
use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// One symbol referenced from a `ScopeResult` — `containing` and
/// `visible` both use this shape so the parent is carried through
/// alongside the ident, with the kind tagged for downstream display.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeSymbol {
    /// `Some(Type)` when the chunk is a method on a concrete type,
    /// `None` for free items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Symbol identifier.
    pub ident: String,
    /// Chunk kind (`fn`, `method`, `struct`, …).
    pub kind: String,
}

/// Result of getting scope information.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeResult {
    /// The file path.
    pub file: String,
    /// The line number.
    pub line: u32,
    /// Symbols that contain this line (scopes we're inside of).
    pub containing: Vec<ScopeSymbol>,
    /// Symbols visible at this location.
    pub visible: Vec<ScopeSymbol>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Get what symbols are visible at a specific line in a file.
pub fn get_scope(db: &Database, path: &str, line: u32) -> Result<ScopeResult> {
    let file = db
        .get_file_by_path(path)?
        .ok_or_else(|| crate::error::RlmError::FileNotFound {
            path: path.to_string(),
        })?;

    let chunks = db.get_chunks_for_file(file.id)?;

    let to_symbol = |c: &crate::domain::chunk::Chunk| ScopeSymbol {
        parent: c.parent.clone(),
        ident: c.ident.clone(),
        kind: c.kind.as_str().to_string(),
    };

    // Find chunks that contain this line
    let containing: Vec<ScopeSymbol> = chunks
        .iter()
        .filter(|c| line >= c.start_line && line <= c.end_line)
        .map(&to_symbol)
        .collect();

    // Find visible symbols: all items defined before this line
    let visible: Vec<ScopeSymbol> = chunks
        .iter()
        .filter(|c| c.start_line <= line)
        .map(&to_symbol)
        .collect();

    let mut result = ScopeResult {
        file: path.to_string(),
        line,
        containing,
        visible,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// `scope <path> <line>` as a [`FileQuery`].
///
/// Scope lives semantically in `application::symbol` because the
/// result enumerates symbols, but its cost model is `SingleFile`
/// (one file read), so it implements `FileQuery` rather than
/// `SymbolQuery`.
pub struct ScopeQuery {
    pub line: u32,
}

impl FileQuery for ScopeQuery {
    type Output = ScopeResult;
    const COMMAND: &'static str = "scope";

    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output> {
        get_scope(db, path, self.line)
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
