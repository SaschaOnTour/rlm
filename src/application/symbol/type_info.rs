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
    /// `Some(Type)` when the picked chunk is a method on a concrete
    /// type, `None` for free functions / module-level items. Surfaces
    /// which `Foo::ident` was selected when polysemy forced a
    /// priority decision (`src/` > default > fixtures), so the caller
    /// can tell whether the picked one is actually what they meant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
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
///
/// When `parent` is `Some(...)`, candidate chunks are filtered to
/// methods of that parent type before the priority pass — keeps the
/// type-info aligned with the caller's polysemy filter so
/// `rlm read --symbol new --parent Foo --metadata` doesn't surface
/// `Bar::new`'s parent.
pub fn get_type_info(db: &Database, symbol: &str, parent: Option<&str>) -> Result<TypeInfoResult> {
    let mut chunks = db.get_chunks_by_ident(symbol)?;
    if let Some(p) = parent {
        chunks.retain(|c| c.parent.as_deref() == Some(p));
    }
    let path_by_id = build_file_path_index(db)?;
    let idx = pick_priority_chunk_index(symbol, &chunks, &path_by_id)?;
    // `swap_remove` is O(1) and the surrounding order is irrelevant
    // (we only consume the picked one).
    let chunk = chunks.swap_remove(idx);
    let file = path_by_id.get(&chunk.file_id).cloned().unwrap_or_default();
    let kind = chunk.kind.as_str().to_string();
    let mut result = TypeInfoResult {
        symbol: symbol.to_string(),
        parent: chunk.parent,
        kind,
        signature: chunk.signature,
        content: chunk.content,
        file,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Build an `O(1)` lookup from `file_id` to file path, sparing
/// callers an `O(chunks × files)` scan in the prioritisation pass.
fn build_file_path_index(db: &Database) -> Result<std::collections::HashMap<i64, String>> {
    Ok(db
        .get_all_files()?
        .into_iter()
        .map(|f| (f.id, f.path))
        .collect())
}

/// Pick the index of the highest-priority chunk (`src/` > default >
/// `fixtures/` / `test`), erroring with `SymbolNotFound` when the
/// list is empty. Returning an index instead of a `&Chunk` lets
/// callers `swap_remove` the chunk and own its fields, sparing
/// boilerplate clones at the construction site.
fn pick_priority_chunk_index(
    symbol: &str,
    chunks: &[crate::domain::chunk::Chunk],
    path_by_id: &std::collections::HashMap<i64, String>,
) -> Result<usize> {
    chunks
        .iter()
        .enumerate()
        .min_by_key(|(_, c)| match path_by_id.get(&c.file_id) {
            Some(path) => priority_for_path(path),
            None => UNKNOWN_FILE_PRIORITY,
        })
        .map(|(idx, _)| idx)
        .ok_or_else(|| crate::error::RlmError::SymbolNotFound {
            ident: symbol.to_string(),
        })
}

/// Priority lattice used by [`pick_priority_chunk`]. Lower wins.
fn priority_for_path(path: &str) -> i32 {
    if path.starts_with("src/") {
        0
    } else if path.contains("fixtures") || path.contains("test") {
        2
    } else {
        1
    }
}

#[cfg(test)]
#[path = "type_info_tests.rs"]
mod tests;
