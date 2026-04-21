//! Tests for `savings.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "savings_tests.rs"] mod tests;`.

use crate::db::Database;
use crate::domain::file::FileRecord;

const TEST_FILE_SIZE_A: u64 = 400;
const TEST_FILE_SIZE_B: u64 = 800;

#[test]
fn scoped_file_stats() {
    let db = Database::open_in_memory().unwrap();
    let f1 = FileRecord::new(
        "src/a.rs".into(),
        "a".into(),
        "rust".into(),
        TEST_FILE_SIZE_A,
    );
    let f2 = FileRecord::new(
        "tests/t.rs".into(),
        "b".into(),
        "rust".into(),
        TEST_FILE_SIZE_B,
    );
    db.upsert_file(&f1).unwrap();
    db.upsert_file(&f2).unwrap();

    let (size, count) = db.get_scoped_file_stats(None).unwrap();
    assert_eq!(count, 2);
    assert_eq!(size, TEST_FILE_SIZE_A + TEST_FILE_SIZE_B);

    let (size, count) = db.get_scoped_file_stats(Some("src/")).unwrap();
    assert_eq!(count, 1);
    assert_eq!(size, TEST_FILE_SIZE_A);

    let (_, count) = db.get_scoped_file_stats(Some("nonexistent/")).unwrap();
    assert_eq!(count, 0);
}
