//! Search operations shared between CLI and MCP.
//!
//! Provides consistent behavior for full-text search across indexed chunks.

use serde::Serialize;

use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::domain::token_budget::{estimate_tokens_str, TokenEstimate};
use crate::error::Result;

/// Approximate number of characters per token for output size estimation.
const MIN_FTS_TOKEN_LENGTH: u64 = 4;

/// Result of a full-text search.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    /// The search results.
    pub results: Vec<SearchHit>,
    /// Number of distinct files represented in `results`.
    ///
    /// Computed from the underlying chunks' `file_id` before the
    /// chunk-to-`SearchHit` mapping drops that information. Consumed
    /// by the savings middleware so the recorded `files_touched`
    /// reflects distinct files, not hit count.
    pub file_count: u64,
    /// Token usage estimate.
    pub tokens: TokenEstimate,
}

/// A single search hit.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    /// The chunk ID.
    pub id: i64,
    /// The kind of the chunk.
    pub kind: String,
    /// The symbol name.
    pub name: String,
    /// The line range [start, end].
    pub lines: (u32, u32),
    /// The content of the chunk.
    pub content: String,
}

/// Perform a full-text search across indexed chunks.
pub fn search_chunks(db: &Database, query: &str, limit: usize) -> Result<SearchResult> {
    use std::collections::HashSet;

    let results = run_fts(db, query, limit)?;

    let file_count = results
        .iter()
        .map(|c| c.file_id)
        .collect::<HashSet<_>>()
        .len() as u64;

    let hits: Vec<SearchHit> = results
        .iter()
        .map(|c| SearchHit {
            id: c.id,
            kind: c.kind.as_str().to_string(),
            name: c.ident.clone(),
            lines: (c.start_line, c.end_line),
            content: c.content.clone(),
        })
        .collect();

    let total_chars: usize = hits.iter().map(|h| h.content.len()).sum();

    Ok(SearchResult {
        results: hits,
        file_count,
        tokens: TokenEstimate::new(
            0,
            estimate_tokens_str(query) + total_chars as u64 / MIN_FTS_TOKEN_LENGTH,
        ),
    })
}

/// Run the FTS5 query with sanitised input, returning raw chunks.
///
/// Inlined from the former `crate::search::fts::search` when the
/// transitional `src/search/` bridge was removed. Keeps the single
/// call site (this module) and its helper in one place.
fn run_fts(db: &Database, query: &str, limit: usize) -> Result<Vec<Chunk>> {
    let sanitized = sanitize_fts_query(query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }
    db.search_fts(&sanitized, limit)
}

/// Sanitize a user query for FTS5.
///
/// Keeps characters that `char::is_alphanumeric` accepts (which is
/// Unicode-wide — letters and digits from any script, so identifiers
/// like `größe` or `日本語` survive), plus whitespace, `_`, and `-`.
/// Drops everything else (quotes, parens, operators, FTS5
/// meta-chars). Splits the cleaned string on whitespace, wraps each
/// term in double quotes so FTS5 treats it as a phrase, and joins the
/// phrases with `OR`. Returns an empty string when the input has no
/// usable characters — the caller short-circuits and returns no hits
/// in that case.
fn sanitize_fts_query(query: &str) -> String {
    let cleaned: String = query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_' || *c == '-')
        .collect();

    let terms: Vec<String> = cleaned
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect();

    terms.join(" OR ")
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
