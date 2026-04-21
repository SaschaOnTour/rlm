//! Dependencies operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting file dependencies using the
//! optimized `get_refs_for_file()` query instead of iterating over chunks.

use std::collections::HashSet;

use serde::Serialize;

use crate::application::FileQuery;
use crate::db::Database;
use crate::domain::chunk::RefKind;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Result of getting dependencies for a file.
#[derive(Debug, Clone, Serialize)]
pub struct DepsResult {
    /// The file path.
    pub file: String,
    /// The list of imports/dependencies.
    pub imports: Vec<String>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Get all imports/dependencies for a file.
///
/// Uses the optimized `get_refs_for_file()` query which joins refs through chunks,
/// rather than iterating over each chunk individually.
pub fn get_deps(db: &Database, path: &str) -> Result<DepsResult> {
    let file = db
        .get_file_by_path(path)?
        .ok_or_else(|| crate::error::RlmError::FileNotFound {
            path: path.to_string(),
        })?;

    // Use the optimized file-level refs query
    let refs = db.get_refs_for_file(file.id)?;

    // Collect unique imports
    let mut imports = HashSet::new();
    for r in refs {
        if r.ref_kind == RefKind::Import {
            imports.insert(r.target_ident);
        }
    }

    // Sort for consistent output
    let mut import_list: Vec<String> = imports.into_iter().collect();
    import_list.sort();

    let mut result = DepsResult {
        file: path.to_string(),
        imports: import_list,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// `deps <path>` as a [`FileQuery`].
pub struct DepsQuery;

impl FileQuery for DepsQuery {
    type Output = DepsResult;
    const COMMAND: &'static str = "deps";

    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output> {
        get_deps(db, path)
    }
}

#[cfg(test)]
#[path = "deps_tests.rs"]
mod tests;
