//! Diff operations shared between CLI and MCP.
//!
//! Provides consistent behavior for comparing indexed versions with current disk versions.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::application::FileQuery;
use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;
use crate::ingest::hasher;

/// Result of comparing a file with its indexed version.
#[derive(Debug, Clone, Serialize)]
pub struct FileDiffResult {
    /// The file path.
    pub file: String,
    /// Whether the file has changed since indexing.
    pub changed: bool,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Result of comparing a symbol with its indexed version.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolDiffResult {
    /// The file path.
    pub file: String,
    /// The symbol name.
    pub symbol: String,
    /// The indexed content.
    pub indexed: String,
    /// The current content.
    pub current: String,
    /// Whether the content has changed.
    pub changed: bool,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Compare a file's current state with its indexed version.
///
/// Returns `changed = true` if:
/// - The file is not in the index, OR
/// - The file's hash differs from the indexed hash
pub fn diff_file(db: &Database, path: &str, project_root: &Path) -> Result<FileDiffResult> {
    let full_path = crate::error::validate_relative_path(path, project_root)?;

    let file = db.get_file_by_path(path)?;

    let current = std::fs::read_to_string(&full_path)?;
    let current_hash = hasher::hash_bytes(current.as_bytes());

    // Unified logic: changed if file not indexed OR hash differs
    let changed = file.is_none_or(|f| f.hash != current_hash);

    let mut result = FileDiffResult {
        file: path.to_string(),
        changed,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Compare a symbol's current state with its indexed version.
///
/// Reads the current file content and extracts the same line range as the indexed chunk.
pub fn diff_symbol(
    db: &Database,
    path: &str,
    symbol: &str,
    project_root: &Path,
) -> Result<SymbolDiffResult> {
    let full_path = crate::error::validate_relative_path(path, project_root)?;

    let chunks = db.get_chunks_by_ident(symbol)?;
    let chunk = chunks
        .first()
        .ok_or_else(|| crate::error::RlmError::SymbolNotFound {
            ident: symbol.to_string(),
        })?;

    let current = std::fs::read_to_string(&full_path)?;

    // Extract current content at the same line range
    let lines: Vec<&str> = current.lines().collect();
    let start = (chunk.start_line as usize).saturating_sub(1);
    let end = (chunk.end_line as usize).min(lines.len());
    let current_content = lines[start..end].join("\n");

    let changed = chunk.content.trim() != current_content.trim();

    let mut result = SymbolDiffResult {
        file: path.to_string(),
        symbol: symbol.to_string(),
        indexed: chunk.content.clone(),
        current: current_content,
        changed,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// `diff <path>` without a symbol filter, as a [`FileQuery`].
pub struct DiffFileQuery {
    pub project_root: PathBuf,
}

impl FileQuery for DiffFileQuery {
    type Output = FileDiffResult;
    const COMMAND: &'static str = "diff";

    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output> {
        diff_file(db, path, &self.project_root)
    }
}

/// `diff <path> --symbol <sym>` as a [`FileQuery`].
pub struct DiffSymbolQuery {
    pub symbol: String,
    pub project_root: PathBuf,
}

impl FileQuery for DiffSymbolQuery {
    type Output = SymbolDiffResult;
    const COMMAND: &'static str = "diff";

    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output> {
        diff_symbol(db, path, &self.symbol, &self.project_root)
    }
}

#[cfg(test)]
#[path = "diff_symbol_tests.rs"]
mod symbol_tests;
#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
