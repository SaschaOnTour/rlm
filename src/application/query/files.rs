//! Files operations shared between CLI and MCP.
//!
//! Provides consistent behavior for listing all files in the project.

use std::path::Path;

use serde::Serialize;

use crate::domain::token_budget::TokenEstimate;
use crate::error::Result;
use crate::ingest::scanner::{DiscoveredFile, Scanner};

/// Estimated number of tokens consumed per file entry in the output.
const TOKENS_PER_FILE_ENTRY: usize = 10;
/// Base token overhead for the wrapping JSON structure of the files result.
const TOKEN_ESTIMATE_BASE_OVERHEAD: usize = 20;

/// Result of listing files.
#[derive(Debug, Clone, Serialize)]
pub struct FilesResult {
    /// The list of discovered files.
    pub results: Vec<DiscoveredFile>,
    /// Summary statistics.
    pub summary: FilesSummary,
    /// Token usage estimate.
    pub tokens: TokenEstimate,
}

/// Summary of file listing.
#[derive(Debug, Clone, Serialize)]
pub struct FilesSummary {
    /// Total number of files.
    pub total: usize,
    /// Number of indexed (supported) files.
    pub indexed: usize,
    /// Number of skipped (unsupported) files.
    pub skipped: usize,
}

/// Filter options for listing files.
#[derive(Debug, Clone, Default)]
pub struct FilesFilter {
    /// Only include files with paths starting with this prefix.
    pub path_prefix: Option<String>,
    /// Only include skipped (unsupported) files.
    pub skipped_only: bool,
    /// Only include indexed (supported) files.
    pub indexed_only: bool,
}

/// List all files in the project with optional filtering.
pub fn list_files(project_root: &Path, filter: FilesFilter) -> Result<FilesResult> {
    let scanner = Scanner::new(project_root);
    let mut files = scanner.scan_all()?;

    // Apply filters
    if let Some(prefix) = &filter.path_prefix {
        files.retain(|f| f.path.starts_with(prefix));
    }
    if filter.skipped_only {
        files.retain(|f| !f.supported);
    }
    if filter.indexed_only {
        files.retain(|f| f.supported);
    }

    // Sort by path for consistent output
    files.sort_by(|a, b| a.path.cmp(&b.path));

    // Calculate summary
    let total = files.len();
    let indexed_count = files.iter().filter(|f| f.supported).count();
    let skipped_count = files.iter().filter(|f| !f.supported).count();

    // Estimate output tokens (rough: ~10 tokens per file entry)
    let out_tokens = (files.len() * TOKENS_PER_FILE_ENTRY + TOKEN_ESTIMATE_BASE_OVERHEAD) as u64;

    Ok(FilesResult {
        results: files,
        summary: FilesSummary {
            total,
            indexed: indexed_count,
            skipped: skipped_count,
        },
        tokens: TokenEstimate::new(0, out_tokens),
    })
}

#[cfg(test)]
#[path = "files_filter_tests.rs"]
mod filter_tests;
#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
