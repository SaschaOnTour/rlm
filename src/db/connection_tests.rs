//! Tests for `connection.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "connection_tests.rs"] mod tests;`.

use super::{Connection, Database};
use tempfile::TempDir;

#[test]
fn open_in_memory_works() {
    let db = Database::open_in_memory().unwrap();
    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn open_creates_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.db");
    let _db = Database::open(&path).unwrap();
    assert!(path.exists());
}

#[test]
fn ancient_schema_is_wiped_and_reseeded() {
    // Simulate an ancient rlm DB: `files` exists but without
    // `doc_comment` / `parse_quality`. A savings table of an
    // unknown old shape is also present — the wipe must drop it
    // so migration 001 recreates it with the current columns
    // rather than leaving the stale shape behind.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("ancient.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE files (id INTEGER PRIMARY KEY, path TEXT);\
             CREATE TABLE chunks (id INTEGER PRIMARY KEY);\
             CREATE TABLE savings (id INTEGER PRIMARY KEY, stale_only_column TEXT);",
        )
        .unwrap();
    }
    let db = Database::open(&path).unwrap();
    // After wipe + migrate, the modern schema is in place.
    assert!(db
        .conn()
        .prepare("SELECT doc_comment FROM chunks LIMIT 0")
        .is_ok());
    assert!(db
        .conn()
        .prepare("SELECT alt_calls FROM savings LIMIT 0")
        .is_ok());
    assert!(db
        .conn()
        .prepare("SELECT mtime_nanos FROM files LIMIT 0")
        .is_ok());
    // The stale-only column from the pre-wipe savings table must
    // be gone — otherwise the wipe preserved the old shape.
    assert!(db
        .conn()
        .prepare("SELECT stale_only_column FROM savings LIMIT 0")
        .is_err());
}
