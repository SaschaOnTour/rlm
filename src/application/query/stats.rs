//! Stats operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting index statistics including quality info.
//! Also hosts the unified `stats_dispatch` / `quality_dispatch` entry points that
//! both CLI and MCP call so neither adapter re-implements the branching logic.

use std::path::Path;

use serde::Serialize;

use crate::application::savings;
use crate::db::Database;
use crate::domain::savings::SavingsReport;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;
use crate::ingest::code::quality_log;
use crate::ingest::code::quality_log::{IssueSummary, QualityIssue};

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

// ── Unified dispatchers (CLI + MCP share these) ────────────────────────

/// Main body of a `stats` response. `#[serde(untagged)]` so adapters
/// can serialise the whole enum with their own formatter and the JSON
/// output matches whichever variant fired.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum StatsBody {
    /// Indexing summary.
    Stats(StatsResult),
    /// Token-savings report (flag `savings = true`).
    Savings(SavingsReport),
}

/// Result of `stats_dispatch`: the main body plus an optional
/// quality-issues side-channel that CLI writes to stderr (MCP drops).
/// The application layer hands back typed values; each adapter owns
/// its own `Formatter` and serialises at the edge.
pub struct StatsDispatchOutput {
    /// Main response body (stats OR savings, never both).
    pub body: StatsBody,
    /// Files-with-parse-warnings summary, populated only on the stats
    /// path when any file has warnings. CLI emits this to stderr;
    /// MCP currently drops it to keep the tool response single-channel.
    pub quality_sidechannel: Option<QualityInfo>,
}

/// Unified entry point for `rlm stats` across CLI and MCP.
///
/// `show_savings = true` returns the token-savings report; false returns
/// the indexing summary (and, for stats-mode only, a quality-issues
/// sidechannel when any file has parse warnings). Both adapters call
/// this single function so branching logic lives here, not in the
/// adapters.
pub fn stats_dispatch(
    db: &Database,
    show_savings: bool,
    since: Option<&str>,
) -> Result<StatsDispatchOutput> {
    if show_savings {
        let report = savings::get_savings_report(db, since)?;
        return Ok(StatsDispatchOutput {
            body: StatsBody::Savings(report),
            quality_sidechannel: None,
        });
    }

    let result = get_stats(db)?;
    let quality_sidechannel = get_quality_info(db)?;
    Ok(StatsDispatchOutput {
        body: StatsBody::Stats(result),
        quality_sidechannel,
    })
}

/// Per-tool confirmation payload emitted when `quality_dispatch` is
/// called with `clear = true`. Carries the boolean flag under its own
/// field so the untagged `QualityBody` picks this variant by structure.
#[derive(Debug, Serialize)]
pub struct QualityClearedAck {
    pub cleared: bool,
}

/// Issues-list payload for `quality_dispatch`. Named struct so serde's
/// untagged selection for [`QualityBody`] works on the field set
/// (`count` + `issues`) rather than a tag.
#[derive(Debug, Serialize)]
pub struct QualityIssues {
    pub count: usize,
    pub issues: Vec<QualityIssue>,
}

/// Main body of a `quality` response.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum QualityBody {
    /// `{"cleared": true}` after a successful `--clear`.
    Cleared(QualityClearedAck),
    /// Issue counts grouped by language / issue type when `summary`.
    Summary(IssueSummary),
    /// Per-issue list (default).
    Issues(QualityIssues),
}

/// Feature flags for [`quality_dispatch`]. Grouped into a struct so the
/// dispatcher signature stays within the SRP parameter budget and both
/// adapters construct the same shape.
#[derive(Debug, Clone, Copy, Default)]
pub struct QualityFlags {
    pub unknown_only: bool,
    pub all: bool,
    pub clear: bool,
    pub summary: bool,
}

/// Unified entry point for `rlm quality` across CLI and MCP.
///
/// Honours the same flag set as the CLI (`unknown_only`, `all`, `clear`,
/// `summary`) and returns a typed [`QualityBody`] so each adapter just
/// serialises it with its own formatter.
pub fn quality_dispatch(log_path: &Path, flags: QualityFlags) -> Result<QualityBody> {
    if flags.clear {
        let logger = quality_log::QualityLogger::new(log_path, true);
        logger.clear()?;
        return Ok(QualityBody::Cleared(QualityClearedAck { cleared: true }));
    }

    let mut issues = quality_log::read_quality_log(log_path)?;
    quality_log::annotate_known_issues(&mut issues);

    // `--all` shows known+unknown; otherwise (default or `--unknown-only`)
    // only unknown issues are surfaced.
    if flags.unknown_only || !flags.all {
        issues = quality_log::filter_unknown(issues);
    }

    if flags.summary {
        Ok(QualityBody::Summary(quality_log::summarize_issues(&issues)))
    } else {
        Ok(QualityBody::Issues(QualityIssues {
            count: issues.len(),
            issues,
        }))
    }
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
