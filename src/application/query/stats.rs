//! Stats operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting index statistics including quality info.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Result of getting index statistics.
#[derive(Debug, Clone, Serialize)]
pub struct StatsResult {
    /// Number of indexed files.
    pub files: u64,
    /// Number of chunks.
    pub chunks: u64,
    /// Number of references.
    pub refs: u64,
    /// Total bytes of indexed files.
    pub total_bytes: u64,
    /// Language breakdown (language, count).
    pub languages: Vec<(String, i64)>,
    /// Oldest indexed timestamp (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_indexed: Option<String>,
    /// Newest indexed timestamp (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest_indexed: Option<String>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Quality information for files.
#[derive(Debug, Clone, Serialize)]
pub struct QualityInfo {
    /// Number of files with parse warnings.
    pub files_with_parse_warnings: usize,
    /// Details of files with quality issues.
    pub files: Vec<QualityFileInfo>,
}

/// Quality info for a single file.
#[derive(Debug, Clone, Serialize)]
pub struct QualityFileInfo {
    /// The file path.
    pub path: String,
    /// The quality status.
    pub quality: String,
}

/// Get index statistics.
pub fn get_stats(db: &Database) -> Result<StatsResult> {
    let stats = db.stats()?;

    let mut result = StatsResult {
        files: stats.file_count,
        chunks: stats.chunk_count,
        refs: stats.ref_count,
        total_bytes: stats.total_bytes,
        languages: stats.languages,
        oldest_indexed: stats.oldest_indexed,
        newest_indexed: stats.newest_indexed,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Get quality information for files with parse issues.
pub fn get_quality_info(db: &Database) -> Result<Option<QualityInfo>> {
    let quality_issues = db.get_files_with_quality_issues()?;

    if quality_issues.is_empty() {
        return Ok(None);
    }

    let files: Vec<QualityFileInfo> = quality_issues
        .into_iter()
        .map(|(path, quality)| QualityFileInfo { path, quality })
        .collect();

    Ok(Some(QualityInfo {
        files_with_parse_warnings: files.len(),
        files,
    }))
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
