//! Tests for `partition.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "partition_tests.rs"] mod tests;`.

use super::{partition_keyword, partition_semantic, partition_uniform, Database};
#[test]
fn uniform_partition() {
    let source = "line1\nline2\nline3\nline4\nline5";
    let parts = partition_uniform(source, 2);
    assert_eq!(parts.len(), 3); // 2+2+1
    assert_eq!(parts[0].start_line, 1);
    assert_eq!(parts[0].end_line, 2);
    assert_eq!(parts[2].start_line, 5);
    assert_eq!(parts[2].end_line, 5);
}

#[test]
fn keyword_partition() {
    let source = "normal\n// TODO: fix\nnormal\n// TODO: another\nend";
    let parts = partition_keyword(source, "TODO").unwrap();
    // Should have partitions separating TODO lines
    assert!(parts.iter().any(|p| p.content.contains("TODO: fix")));
    assert!(parts.iter().any(|p| p.content.contains("TODO: another")));
}

#[test]
fn semantic_partition_fallback() {
    let db = Database::open_in_memory().unwrap();
    let source = "line1\nline2\nline3";
    // No file in DB, should fallback to uniform
    let parts = partition_semantic(&db, "nonexistent.rs", source).unwrap();
    assert!(!parts.is_empty());
}
