//! Variable-limit-safe batched IN-list queries.
//!
//! SQLite has a hard compile-time ceiling on host parameters per
//! prepared statement: `SQLITE_MAX_VARIABLE_NUMBER`. Historically 999;
//! since SQLite 3.32 (May 2020) it's 32766. rusqlite 0.38 with
//! `bundled` ships ≥ 3.50 so the practical limit is 32766. Either way
//! it's finite — a `WHERE x IN (?, ?, …, ?)` query whose placeholder
//! count exceeds it errors at `prepare()` time with `SQLITE_RANGE`
//! ("too many SQL variables").
//!
//! [`query_batched_in`] funnels every dynamic `IN`-list query through
//! one chunking loop so the failure mode can't surface in user-facing
//! operations on large projects. New callers should reach for this
//! helper instead of building raw IN-clauses; the
//! `no_unbatched_in_lists` rustqual rule enforces that
//! `rusqlite::params_from_iter` only appears here (and in tests).
//!
//! These live as free functions on purpose: hanging them on
//! `Database` would push that struct's LCOM4 over the SRP threshold
//! (Database already hosts savings/chunks/files/refs/search/stats
//! method clusters). The function-with-explicit-`&Database` form is
//! idiomatic in Rust (cf. `std::slice` adapters) and keeps the
//! struct's cohesion budget for storage concerns.

use std::collections::HashSet;
use std::hash::Hash;

use rusqlite::{params_from_iter, ToSql};

use crate::db::Database;
use crate::error::Result;

/// Default chunk size used by [`query_batched_in`].
///
/// Picked well below the SQLite 3.32+ ceiling of 32766 so a single
/// query stays cheap to prepare and there's headroom for ports that
/// might still see the historical 999 limit. Two extra placeholders
/// might be reserved elsewhere in the query (`LIMIT ?`, etc.) — 999
/// leaves room.
pub(crate) const SQLITE_VAR_LIMIT: usize = 999;

/// Run an IN-list `SELECT` in batches that stay under the SQLite
/// host-parameter limit. The closures decouple the per-batch shape:
///
/// * `sql_for(n)` builds the SQL with exactly `n` placeholders —
///   typically a `format!("… IN ({placeholders})")` over
///   `repeat("?", n).join(",")`. Called once per batch.
/// * `row_mapper(row)` maps one result row to the caller's output
///   type. Called once per result row.
///
/// **Set semantics**: input items are deduplicated before batching
/// so cross-batch duplicates can't double-return the same row. A
/// single-query `IN (?, ?)` with the same id twice returns one row
/// (SQL set semantics); the batched form preserves that. The
/// `Eq + Hash` bound makes the dedup `O(n)` rather than `O(n²)`.
///
/// **Result order**: rows arrive in the order SQLite emits per
/// batch, with per-batch result sets concatenated in the order the
/// helper iterates its input chunks. Callers that need a stable
/// total order should `sort` (or build a `HashMap` keyed by id) on
/// the returned `Vec`; nothing in this helper rearranges per-batch
/// output.
///
/// The chunk size is fixed at [`SQLITE_VAR_LIMIT`]; tests use
/// [`query_batched_in_with_limit`] to drive smaller boundaries.
pub(crate) fn query_batched_in<P, R, S, M>(
    db: &Database,
    items: &[P],
    sql_for: S,
    row_mapper: M,
) -> Result<Vec<R>>
where
    P: ToSql + Eq + Hash + Clone,
    S: Fn(usize) -> String,
    M: Fn(&rusqlite::Row<'_>) -> rusqlite::Result<R>,
{
    query_batched_in_with_limit(db, items, SQLITE_VAR_LIMIT, sql_for, row_mapper)
}

/// Test/edge-case variant of [`query_batched_in`] that takes an
/// explicit batch size. Lets tests drive the boundary-crossing path
/// with N=3 instead of needing 1000+ fixtures. Production callers
/// use the wrapper above.
pub(crate) fn query_batched_in_with_limit<P, R, S, M>(
    db: &Database,
    items: &[P],
    limit: usize,
    sql_for: S,
    row_mapper: M,
) -> Result<Vec<R>>
where
    P: ToSql + Eq + Hash + Clone,
    S: Fn(usize) -> String,
    M: Fn(&rusqlite::Row<'_>) -> rusqlite::Result<R>,
{
    debug_assert!(limit > 0, "batch limit must be positive");
    // Dedup before batching so cross-batch duplicates can't
    // double-return — see the doc on `query_batched_in` for why
    // this matches single-query `IN(...)` set semantics. Input
    // order isn't promised (we say so in the docs), so a HashSet
    // round-trip is the simplest correct dedup.
    let mut seen: HashSet<P> = HashSet::with_capacity(items.len());
    let mut unique: Vec<P> = Vec::with_capacity(items.len());
    for item in items {
        if seen.insert(item.clone()) {
            unique.push(item.clone());
        }
    }
    let mut out = Vec::new();
    for batch in unique.chunks(limit) {
        let sql = sql_for(batch.len());
        let mut stmt = db.conn().prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(batch), &row_mapper)?;
        for r in rows {
            out.push(r?);
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "batched_tests.rs"]
mod tests;
