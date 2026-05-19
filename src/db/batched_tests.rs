//! Tests for [`query_batched_in`].
//!
//! Verifies chunking behavior at the boundary (`limit < input` vs
//! `limit >= input`), input-order preservation across batches, and
//! the empty-input fast path. Driven via the
//! `query_batched_in_with_limit` test seam so we don't need 33k-row
//! fixtures to exercise the chunk-crossing path.

use super::{query_batched_in, query_batched_in_with_limit, SQLITE_VAR_LIMIT};
use crate::db::Database;
use crate::domain::file::FileRecord;

/// Insert N files named `f0.rs`, `f1.rs`, … so we have a stable
/// integer id space we can query with `IN (?, ?, …)`.
fn seed_files(db: &Database, n: usize) -> Vec<i64> {
    (0..n)
        .map(|i| {
            let f = FileRecord::new(format!("src/f{i}.rs"), "h".into(), "rust".into(), i as u64);
            db.upsert_file(&f).unwrap()
        })
        .collect()
}

/// SQL template + row mapper for the trivial "give me back the ids
/// you found" query. Keeps these tests focused on chunking, not
/// row shape.
fn id_only_sql(n: usize) -> String {
    let placeholders = std::iter::repeat_n("?", n).collect::<Vec<_>>().join(",");
    format!("SELECT id FROM files WHERE id IN ({placeholders}) ORDER BY id")
}

fn id_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<i64> {
    row.get(0)
}

#[test]
fn returns_empty_for_empty_input_without_running_a_query() {
    let db = Database::open_in_memory().unwrap();
    let out: Vec<i64> = query_batched_in(&db, &[] as &[i64], id_only_sql, id_from_row).unwrap();
    assert!(out.is_empty());
}

#[test]
fn single_batch_when_input_fits_under_limit() {
    let db = Database::open_in_memory().unwrap();
    let ids = seed_files(&db, 5);
    let out: Vec<i64> =
        query_batched_in_with_limit(&db, &ids, 999, id_only_sql, id_from_row).unwrap();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(out, sorted);
}

#[test]
fn splits_into_multiple_batches_when_input_exceeds_limit() {
    let db = Database::open_in_memory().unwrap();
    // 7 ids with batch limit 3 → batches of [3, 3, 1].
    let ids = seed_files(&db, 7);
    let out: Vec<i64> =
        query_batched_in_with_limit(&db, &ids, 3, id_only_sql, id_from_row).unwrap();
    // Per-batch results are concatenated in input-slice order; each
    // batch internally orders by id, so for sequential inserts the
    // overall sequence is identical to the sorted input.
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(out, sorted);
    assert_eq!(out.len(), 7);
}

#[test]
fn batch_boundary_exactly_at_limit_does_not_emit_empty_trailing_batch() {
    let db = Database::open_in_memory().unwrap();
    // 6 ids, limit 3 → batches of [3, 3]. The .chunks() iterator
    // never yields an empty tail, but pin it explicitly so a future
    // refactor that uses %-based math can't introduce one.
    let ids = seed_files(&db, 6);
    let out: Vec<i64> =
        query_batched_in_with_limit(&db, &ids, 3, id_only_sql, id_from_row).unwrap();
    assert_eq!(out.len(), 6);
}

// SQLite pre-3.32 had a 999 var ceiling; SQLITE_VAR_LIMIT sits at
// or below that so even legacy ports work. Compile-time assertion
// keeps the floor honest without needing a runtime test that has
// no subject-under-test (and that clippy would flag as a const
// expression anyway).
const _: () = assert!(
    SQLITE_VAR_LIMIT <= 999,
    "SQLITE_VAR_LIMIT must stay at or under the legacy SQLite ceiling \
     of 999 host parameters",
);

#[test]
fn row_mapper_receives_each_row_once_across_batches() {
    use std::cell::Cell;
    let db = Database::open_in_memory().unwrap();
    let ids = seed_files(&db, 5);
    let calls = Cell::new(0);
    let _: Vec<i64> = query_batched_in_with_limit(&db, &ids, 2, id_only_sql, |row| {
        calls.set(calls.get() + 1);
        row.get(0)
    })
    .unwrap();
    // 5 ids should fire the row mapper 5 times, not 5 * batch count.
    assert_eq!(calls.get(), 5);
}

/// Production sanity-check: the public `query_batched_in` (no
/// explicit limit) uses [`SQLITE_VAR_LIMIT`] and still returns every
/// row. We don't need to insert SQLITE_VAR_LIMIT+1 files for that —
/// the helper-with-limit tests above cover the boundary; this just
/// pins that the no-limit wrapper delegates correctly.
#[test]
fn public_wrapper_returns_all_matching_rows() {
    let db = Database::open_in_memory().unwrap();
    let ids = seed_files(&db, 50);
    let out: Vec<i64> = query_batched_in(&db, &ids, id_only_sql, id_from_row).unwrap();
    assert_eq!(out.len(), 50);
}
