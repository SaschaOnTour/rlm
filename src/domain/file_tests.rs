//! Tests for `file.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "file_tests.rs"] mod tests;`.

use super::FileRecord;
#[test]
fn file_record_new_sets_zero_id() {
    const FILE_SIZE: u64 = 1024;

    let f = FileRecord::new(
        "src/main.rs".into(),
        "abc123".into(),
        "rust".into(),
        FILE_SIZE,
    );
    assert_eq!(f.id, 0);
    assert_eq!(f.path, "src/main.rs");
    assert_eq!(f.lang, "rust");
    assert_eq!(f.size_bytes, FILE_SIZE);
    assert_eq!(f.mtime_nanos, 0);
}

#[test]
fn file_record_with_mtime_sets_field() {
    const FILE_SIZE: u64 = 2048;
    const SAMPLE_MTIME: i64 = 1_700_000_000;

    let f = FileRecord::with_mtime(
        "src/lib.rs".into(),
        "def456".into(),
        "rust".into(),
        FILE_SIZE,
        SAMPLE_MTIME,
    );
    assert_eq!(f.mtime_nanos, SAMPLE_MTIME);
    assert_eq!(f.size_bytes, FILE_SIZE);
}
