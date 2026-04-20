//! Verify operations shared between CLI and MCP.
//!
//! Provides consistent behavior for verifying index integrity with proper
//! error handling (no silent failures).

use std::path::Path;

use serde::Serialize;

use crate::db::queries::VerifyReport;
use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Result of fixing integrity issues.
#[derive(Debug, Clone, Serialize)]
pub struct FixResult {
    /// Whether fixes were applied.
    pub fixed: bool,
    /// Number of orphan chunks deleted.
    pub orphan_chunks_deleted: u64,
    /// Number of orphan refs deleted.
    pub orphan_refs_deleted: u64,
    /// Number of missing files removed from index.
    pub missing_files_removed: u64,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Verify index integrity and check for missing files on disk.
///
/// Checks:
/// - `SQLite` integrity
/// - Orphan chunks (`file_id` points to deleted file)
/// - Orphan refs (`chunk_id` points to deleted chunk)
/// - Files in index that no longer exist on disk
pub fn verify_index(db: &Database, project_root: &Path) -> Result<VerifyReport> {
    let mut report = db.verify_integrity()?;

    // Check for files that no longer exist on disk
    let indexed_paths = db.get_all_file_paths()?;
    for path in &indexed_paths {
        let full_path = project_root.join(path);
        if !full_path.exists() {
            report.missing_files += 1;
            report.missing_file_paths.push(path.clone());
        }
    }

    report.tokens = estimate_output_tokens(&report);
    Ok(report)
}

/// Fix integrity issues by deleting orphans and removing missing files.
///
/// Unlike the MCP version that used `unwrap_or(false)` which silently ignored errors,
/// this function properly propagates errors.
pub fn fix_integrity(db: &Database, report: &VerifyReport) -> Result<FixResult> {
    // Fix orphans (refs first, then chunks)
    let (chunks_fixed, refs_fixed) = db.fix_orphans()?;

    // Remove missing files from index - propagate errors instead of ignoring them
    let mut files_removed = 0u64;
    for path in &report.missing_file_paths {
        if db.delete_file_by_path(path)? {
            files_removed += 1;
        }
    }

    let mut result = FixResult {
        fixed: true,
        orphan_chunks_deleted: chunks_fixed,
        orphan_refs_deleted: refs_fixed,
        missing_files_removed: files_removed,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
#[path = "verify_fix_tests.rs"]
mod fix_tests;
#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
