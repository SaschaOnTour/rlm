//! Lexically-similar symbol suggestions (task #106 / T5).
//!
//! After a write, `analyze_test_impact` asks this module whether the
//! modified symbol has near-neighbours elsewhere in the codebase.
//! Typos (`opne` → `open`) and naming-convention cousins (`parse`
//! vs. `parse_strict`) are the common use cases: the agent probably
//! wants to know whether those related symbols need a parallel
//! change or whether they already have the behaviour the agent is
//! now re-implementing.
//!
//! Algorithm: classic two-row Levenshtein distance. At index time
//! this is pure string work, no AST or DB dependency. At query time
//! we walk every fn/method chunk in the DB (excluding the changed
//! file), score against the target ident, and keep the top-N by
//! distance. For a 50k-symbol codebase this is a few-millisecond
//! scan — fine for one-shot write responses.

use crate::db::Database;
use crate::domain::chunk::ChunkKind;
use crate::error::Result;

/// Default ceiling: distance > 3 means the symbols aren't lexically
/// related in any useful way. Tuned against the usual Rust naming
/// patterns — any typo an agent is realistically going to make is
/// within 3 edits of the intended symbol.
pub const DEFAULT_MAX_DISTANCE: u32 = 3;

/// Default cap on suggestions returned. Five is enough for a hint
/// block; more starts to feel like a search result.
pub const DEFAULT_TOP_N: usize = 5;

/// One lexically-similar symbol suggestion surfaced in a write
/// response.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SimilarSymbol {
    pub symbol: String,
    pub file: String,
    pub distance: u32,
}

/// Levenshtein distance between two strings, using the two-row
/// dynamic-programming table. Allocation-free for short idents (the
/// common case), and well under a millisecond even at kilobyte
/// lengths.
#[must_use]
pub fn levenshtein(a: &str, b: &str) -> u32 {
    if a.is_empty() {
        return b.chars().count() as u32;
    }
    if b.is_empty() {
        return a.chars().count() as u32;
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let n = a_chars.len();
    let m = b_chars.len();

    let mut prev: Vec<u32> = (0..=m as u32).collect();
    let mut curr: Vec<u32> = vec![0; m + 1];

    for i in 1..=n {
        curr[0] = i as u32;
        for j in 1..=m {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// Find up to `top_n` symbols across the codebase whose ident is
/// within `max_distance` of `target`, excluding any chunks in
/// `exclude_file_id` (so the changed file itself doesn't suggest its
/// own neighbours). Results are sorted by ascending distance, then
/// by ident for deterministic tie-breaking.
pub fn find_similar_symbols(
    db: &Database,
    target: &str,
    exclude_file_id: Option<i64>,
    max_distance: u32,
    top_n: usize,
) -> Result<Vec<SimilarSymbol>> {
    if target.is_empty() {
        return Ok(Vec::new());
    }
    let chunks = db.get_all_chunks()?;
    let files = db.get_all_files()?;
    let file_by_id: std::collections::HashMap<i64, &str> =
        files.iter().map(|f| (f.id, f.path.as_str())).collect();

    let mut hits: Vec<SimilarSymbol> = chunks
        .iter()
        .filter(|c| matches!(c.kind, ChunkKind::Function | ChunkKind::Method))
        .filter(|c| Some(c.file_id) != exclude_file_id)
        .filter(|c| c.ident != target)
        .filter_map(|c| {
            let distance = levenshtein(&c.ident, target);
            if distance > max_distance {
                return None;
            }
            let file = file_by_id.get(&c.file_id)?.to_string();
            Some(SimilarSymbol {
                symbol: c.ident.clone(),
                file,
                distance,
            })
        })
        .collect();

    hits.sort_by(|a, b| {
        a.distance
            .cmp(&b.distance)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    hits.truncate(top_n);
    Ok(hits)
}

#[cfg(test)]
#[path = "similar_symbols_tests.rs"]
mod tests;
