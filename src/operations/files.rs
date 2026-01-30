//! Files operations shared between CLI and MCP.
//!
//! Provides consistent behavior for listing all files in the project.

use std::path::Path;

use serde::Serialize;

use crate::error::Result;
use crate::ingest::scanner::{DiscoveredFile, Scanner};
use crate::models::token_estimate::TokenEstimate;

/// Result of listing files.
#[derive(Debug, Clone, Serialize)]
pub struct FilesResult {
    /// The list of discovered files.
    #[serde(rename = "r")]
    pub results: Vec<DiscoveredFile>,
    /// Summary statistics.
    #[serde(rename = "s")]
    pub summary: FilesSummary,
    /// Token usage estimate.
    #[serde(rename = "t")]
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
        files.retain(|f| f.relative_path.starts_with(prefix));
    }
    if filter.skipped_only {
        files.retain(|f| !f.supported);
    }
    if filter.indexed_only {
        files.retain(|f| f.supported);
    }

    // Sort by path for consistent output
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    // Calculate summary
    let total = files.len();
    let indexed_count = files.iter().filter(|f| f.supported).count();
    let skipped_count = files.iter().filter(|f| !f.supported).count();

    // Estimate output tokens (rough: ~10 tokens per file entry)
    let out_tokens = (files.len() * 10 + 20) as u64;

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
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn list_files_basic() {
        let tmp = TempDir::new().unwrap();

        // Create some files
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(tmp.path().join("lib.rs"), "// lib").unwrap();
        std::fs::write(tmp.path().join("README.md"), "# README").unwrap();

        let result = list_files(tmp.path(), FilesFilter::default()).unwrap();
        assert_eq!(result.summary.total, 3);
        assert!(result.summary.indexed > 0);
    }

    #[test]
    fn list_files_with_path_filter() {
        let tmp = TempDir::new().unwrap();

        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "").unwrap();
        std::fs::write(tmp.path().join("tests/test.rs"), "").unwrap();

        let filter = FilesFilter {
            path_prefix: Some("src".into()),
            ..Default::default()
        };
        let result = list_files(tmp.path(), filter).unwrap();
        assert_eq!(result.summary.total, 1);
        assert!(result.results[0].relative_path.starts_with("src"));
    }

    #[test]
    fn list_files_skipped_only() {
        let tmp = TempDir::new().unwrap();

        std::fs::write(tmp.path().join("main.rs"), "").unwrap(); // supported
        std::fs::write(tmp.path().join("data.xyz"), "").unwrap(); // unsupported

        let filter = FilesFilter {
            skipped_only: true,
            ..Default::default()
        };
        let result = list_files(tmp.path(), filter).unwrap();
        // Only unsupported files
        for f in &result.results {
            assert!(!f.supported);
        }
    }

    #[test]
    fn list_files_indexed_only() {
        let tmp = TempDir::new().unwrap();

        std::fs::write(tmp.path().join("main.rs"), "").unwrap(); // supported
        std::fs::write(tmp.path().join("data.xyz"), "").unwrap(); // unsupported

        let filter = FilesFilter {
            indexed_only: true,
            ..Default::default()
        };
        let result = list_files(tmp.path(), filter).unwrap();
        // Only supported files
        for f in &result.results {
            assert!(f.supported);
        }
    }
}
