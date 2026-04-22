//! Search operations shared between CLI and MCP.
//!
//! Provides consistent behavior for full-text search across indexed chunks.

use serde::Serialize;

use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::domain::token_budget::{estimate_tokens_str, TokenEstimate};
use crate::error::{Result, RlmError};

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
///
/// `content` is `Some(..)` under [`FieldsMode::Full`] (default — the
/// agent gets the code in one call, no follow-up `rlm read` needed) and
/// `None` under [`FieldsMode::Minimal`] (the agent just wanted names /
/// file paths). The `skip_serializing_if` attribute keeps the JSON
/// payload small when `content` is absent.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub lines: (u32, u32),
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Which fields to populate on every [`SearchHit`] — see
/// `docs/bugs/search-fields-projection.md` for the break-even
/// analysis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FieldsMode {
    /// Default: include the full chunk content so the caller doesn't
    /// need a second `rlm read`. Optimal when the agent plans to read
    /// at least one of the hits.
    #[default]
    Full,
    /// Drop `content`, keep metadata (id, kind, name, lines). Optimal
    /// for "does X exist?" / "which files?" where only identifiers
    /// matter; per-call output drops from ~5k tokens to a few hundred.
    Minimal,
}

impl FieldsMode {
    /// Parse from optional `&str`, defaulting to `Full` when the
    /// adapter didn't pass one. Unknown values error at the adapter
    /// edge so typos surface instead of silently falling back.
    pub fn from_optional(s: Option<&str>) -> Result<Self> {
        match s {
            None => Ok(Self::default()),
            Some("full") => Ok(Self::Full),
            Some("minimal") => Ok(Self::Minimal),
            Some(other) => Err(RlmError::InvalidPattern {
                pattern: other.to_string(),
                reason: "unknown fields mode — use 'full' or 'minimal'".into(),
            }),
        }
    }
}

/// Perform a full-text search across indexed chunks. Convenience wrapper
/// around [`search_chunks_with_fields`] using the [`FieldsMode::Full`]
/// default so behavioural tests stay compact.
#[cfg(test)]
pub(crate) fn search_chunks(db: &Database, query: &str, limit: usize) -> Result<SearchResult> {
    search_chunks_with_fields(db, query, limit, FieldsMode::Full)
}

/// Perform a full-text search with an explicit projection mode.
// qual:api
pub fn search_chunks_with_fields(
    db: &Database,
    query: &str,
    limit: usize,
    fields: FieldsMode,
) -> Result<SearchResult> {
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
            content: match fields {
                FieldsMode::Full => Some(c.content.clone()),
                FieldsMode::Minimal => None,
            },
        })
        .collect();

    let total_chars: usize = hits
        .iter()
        .map(|h| h.content.as_deref().map_or(0, str::len))
        .sum();

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
/// Produces an FTS5 query string with sensible defaults:
///
/// * **AND by default**: space-separated bare tokens pass through
///   unchanged. FTS5 interprets that as AND — every query tool's
///   default, and the only behaviour that narrows a search as you
///   add terms.
/// * **Explicit OR**: the word `OR` survives, so users opt in to
///   broader matches (`auth OR login`).
/// * **Phrase queries**: balanced `"..."` survives so FTS5 does a
///   contiguous-token match.
/// * **Unicode-wide identifiers** (e.g. `größe`, `日本語`, `authenticate_user`).
/// * **Injection-safe**: FTS5 meta-characters outside the allowed set
///   become whitespace (= word break = AND separator). An unbalanced
///   trailing `"` is stripped so FTS5 never errors on it.
///
/// Returns an empty string when the input has no usable tokens;
/// the caller short-circuits to "no hits".
fn sanitize_fts_query(query: &str) -> String {
    // Whitelist characters the FTS5 parser needs. `"` enables phrase
    // queries; `*` enables prefix matches (`foo*`). Everything else
    // collapses to space (a word break → AND separator).
    let mapped: String = query
        .chars()
        .map(|c| match c {
            c if c.is_alphanumeric() => c,
            ' ' | '\t' | '\n' | '\r' => ' ',
            '_' | '-' | '"' | '*' => c,
            _ => ' ',
        })
        .collect();

    // Balance quotes: if the total is odd, strip the last `"`. Doing
    // this on the MAPPED string (not the original) means non-ASCII
    // quotes that became spaces don't throw off the count.
    let balanced = balance_quotes(&mapped);

    // Tokenise + drop whitespace.
    let tokens: Vec<&str> = balanced.split_whitespace().collect();

    // Whitelisting `*` / `OR` lets users opt into prefix + disjunction
    // queries, but a bare `*` or a dangling / repeated `OR` is a
    // syntactically invalid FTS5 query — it would come back as an
    // opaque SQLite error instead of "no hits". Fix that here so the
    // sanitizer's contract is "everything past here is a parseable
    // FTS5 expression".
    clean_operator_tokens(&tokens).join(" ")
}

/// Post-process tokens so the resulting query is a valid FTS5 expression.
///
/// Rules (applied in order):
/// 1. Drop any standalone `*` — FTS5 only treats `*` as a meaningful
///    marker when suffixed to another token (`foo*`). Bare `*` is a
///    syntax error.
/// 2. Collapse consecutive `OR` tokens to one, and strip leading /
///    trailing `OR` — a dangling operator has no operand.
/// 3. If nothing content-bearing is left (only operators), return an
///    empty vec — the caller then short-circuits to "no hits" instead
///    of letting FTS5 raise.
fn clean_operator_tokens(tokens: &[&str]) -> Vec<String> {
    let without_bare_star: Vec<&str> = tokens.iter().copied().filter(|t| *t != "*").collect();

    let mut dedup: Vec<String> = Vec::with_capacity(without_bare_star.len());
    for &t in &without_bare_star {
        if t == "OR" && dedup.last().is_some_and(|s| s == "OR") {
            continue;
        }
        dedup.push(t.to_string());
    }

    while dedup.first().is_some_and(|s| s == "OR") {
        dedup.remove(0);
    }
    while dedup.last().is_some_and(|s| s == "OR") {
        dedup.pop();
    }

    // Only operators left (shouldn't happen after the stripping above,
    // but belt-and-braces — an all-OR input `"OR OR OR"` dedupes to
    // `"OR"`, which both leading- and trailing-strip to empty).
    if dedup.iter().all(|t| t == "OR") {
        return Vec::new();
    }
    dedup
}

/// Remove the last `"` if the string contains an odd number of them.
/// Keeps the rest of the input intact. Extracted so the quote-parity
/// logic has one place to live (and to test) instead of being inlined.
fn balance_quotes(s: &str) -> String {
    let count = s.chars().filter(|&c| c == '"').count();
    if count % 2 == 0 {
        return s.to_string();
    }
    match s.rfind('"') {
        Some(idx) => {
            let mut out = String::with_capacity(s.len() - 1);
            out.push_str(&s[..idx]);
            out.push_str(&s[idx + 1..]);
            out
        }
        None => s.to_string(),
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
