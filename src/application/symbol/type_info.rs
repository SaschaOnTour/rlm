//! Type info operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting type information about symbols,
//! including prioritization of chunks from src/ over fixtures/tests.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Priority value assigned to chunks whose file record is unknown,
/// ensuring they sort below src/ (0), default (1), and fixtures/tests (2).
const UNKNOWN_FILE_PRIORITY: i32 = 3;

/// Result of getting type information for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct TypeInfoResult {
    /// The symbol name.
    pub symbol: String,
    /// The kind of the symbol (fn, struct, class, etc.).
    pub kind: String,
    /// The signature if available.
    pub signature: Option<String>,
    /// The full content of the symbol.
    pub content: String,
    /// The file path where the symbol is defined.
    pub file: String,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Get type information for a symbol.
///
/// Prioritizes chunks from:
/// 1. `src/` directory (highest priority)
/// 2. Default directories
/// 3. `fixtures/` or `test` directories (lowest priority)
///
/// This ensures consistent results when a symbol exists in multiple locations
/// (e.g., both in source and test fixtures).
pub fn get_type_info(db: &Database, symbol: &str) -> Result<TypeInfoResult> {
    let chunks = db.get_chunks_by_ident(symbol)?;

    if chunks.is_empty() {
        return Err(crate::error::RlmError::SymbolNotFound {
            ident: symbol.to_string(),
        });
    }

    // Build file lookup for O(1) access instead of O(chunks * files)
    let files = db.get_all_files()?;
    let file_map: std::collections::HashMap<i64, &crate::domain::file::FileRecord> =
        files.iter().map(|f| (f.id, f)).collect();

    // Prioritize chunks: src/ > default > fixtures/tests
    let chunk = chunks
        .iter()
        .min_by_key(|c| match file_map.get(&c.file_id) {
            Some(f) => {
                if f.path.starts_with("src/") {
                    0 // Highest priority for src/
                } else if f.path.contains("fixtures") || f.path.contains("test") {
                    2 // Lowest priority for fixtures/tests
                } else {
                    1 // Medium priority for everything else
                }
            }
            None => UNKNOWN_FILE_PRIORITY, // Unknown files get lowest priority
        })
        .ok_or_else(|| crate::error::RlmError::SymbolNotFound {
            ident: symbol.to_string(),
        })?;

    let file_path = file_map
        .get(&chunk.file_id)
        .map(|f| f.path.clone())
        .unwrap_or_default();

    let mut result = TypeInfoResult {
        symbol: symbol.to_string(),
        kind: chunk.kind.as_str().to_string(),
        signature: chunk.signature.clone(),
        content: chunk.content.clone(),
        file: file_path,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
#[path = "type_info_tests.rs"]
mod tests;
