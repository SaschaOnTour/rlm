//! File-level diff tests for `diff.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "diff_tests.rs"] mod tests;`.
//!
//! Symbol-level diff tests live in the sibling `diff_symbol_tests.rs`.

use super::super::fixtures::setup_test_db_and_dir;
use super::{diff_file, hasher};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;

#[test]
fn diff_file_unchanged() {
    let (db, tmp) = setup_test_db_and_dir();

    // Create file on disk
    let file_path = tmp.path().join("test.rs");
    let content = "fn main() {}";
    std::fs::write(&file_path, content).unwrap();

    // Index with matching hash
    let hash = hasher::hash_bytes(content.as_bytes());
    let file = FileRecord::new("test.rs".into(), hash, "rust".into(), content.len() as u64);
    db.upsert_file(&file).unwrap();

    let result = diff_file(&db, "test.rs", tmp.path()).unwrap();
    assert!(!result.changed);
}

#[test]
fn diff_file_changed() {
    let (db, tmp) = setup_test_db_and_dir();

    // Create file on disk
    let file_path = tmp.path().join("test.rs");
    std::fs::write(&file_path, "fn main() { new code }").unwrap();

    // Index with different hash
    let file = FileRecord::new(
        "test.rs".into(),
        "oldhash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    db.upsert_file(&file).unwrap();

    let result = diff_file(&db, "test.rs", tmp.path()).unwrap();
    assert!(result.changed);
}

#[test]
fn diff_file_not_indexed() {
    let (db, tmp) = setup_test_db_and_dir();

    // Create file on disk but don't index it
    let file_path = tmp.path().join("test.rs");
    std::fs::write(&file_path, "fn main() {}").unwrap();

    let result = diff_file(&db, "test.rs", tmp.path()).unwrap();
    assert!(result.changed); // Not indexed = changed
}
