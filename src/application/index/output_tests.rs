//! Tests for `index.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "index_tests.rs"] mod tests;`.

use super::IndexOutput;
use crate::application::index::IndexResult;

#[test]
fn index_output_from_result() {
    let result = IndexResult {
        files_scanned: 100,
        files_indexed: 80,
        files_skipped: 20,
        chunks_created: 500,
        refs_created: 200,
        skipped_unsupported: 10,
        skipped_too_large: 2,
        skipped_non_utf8: 3,
        skipped_io_error: 1,
        skipped_unchanged: 4,
        deleted_from_index: 5,
    };

    let output: IndexOutput = result.into();
    assert_eq!(output.files_scanned, 100);
    assert_eq!(output.files_indexed, 80);
    assert_eq!(output.skipped_unsupported, 10);
}

#[test]
fn index_output_serialization_skips_zeros() {
    let result = IndexResult {
        files_scanned: 10,
        files_indexed: 10,
        files_skipped: 0,
        chunks_created: 50,
        refs_created: 20,
        skipped_unsupported: 0,
        skipped_too_large: 0,
        skipped_non_utf8: 0,
        skipped_io_error: 0,
        skipped_unchanged: 0,
        deleted_from_index: 0,
    };

    let output: IndexOutput = result.into();
    let json = serde_json::to_string(&output).unwrap();

    // Zero fields should not appear in JSON
    assert!(!json.contains("skipped_unsupported"));
    assert!(!json.contains("skipped_too_large"));
    assert!(!json.contains("deleted_from_index"));
}
