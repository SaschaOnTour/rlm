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
fn open_required_returns_not_found_for_missing_path() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("does_not_exist.db");
    match Database::open_required(&missing) {
        Err(crate::error::RlmError::IndexNotFound) => {}
        Ok(_) => panic!("missing path must not open successfully"),
        Err(e) => panic!("expected IndexNotFound, got error: {e}"),
    }
}

#[test]
fn open_required_opens_existing_db() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("exists.db");
    // Create the DB first so open_required can find it.
    assert!(Database::open(&path).is_ok());
    assert!(Database::open_required(&path).is_ok());
}

#[cfg(unix)]
#[test]
fn open_required_propagates_io_error_instead_of_misclassifying() {
    // Regression: `Path::exists()` returns false for permission / IO errors,
    // which would misleadingly map to IndexNotFound. `metadata()` + kind
    // matching surfaces the real error. Skipped when run as root (which
    // bypasses Unix permission checks).
    use std::os::unix::fs::PermissionsExt;

    /// Unix mode: owner+group+world no access, to deny traversal into the dir.
    const LOCKED_MODE: u32 = 0o000;
    /// Restore to standard rwxr-xr-x so `TempDir` cleanup can descend.
    const RESTORED_MODE: u32 = 0o755;

    let tmp = TempDir::new().unwrap();
    let locked = tmp.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    let inner = locked.join("db");
    std::fs::write(&inner, b"placeholder").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(LOCKED_MODE)).unwrap();

    let result = Database::open_required(&inner);

    // Restore permissions so TempDir cleanup works regardless of assertion.
    let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(RESTORED_MODE));

    if matches!(result, Err(crate::error::RlmError::IndexNotFound)) {
        panic!("IO error must not be classified as IndexNotFound");
    }
    // Err(other) is the expected correct behavior; Ok is only possible
    // as root (permission bypass) and is treated as inconclusive.
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
