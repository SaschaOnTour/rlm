//! Tests for filter-dispatch behavior of `list_files`.
//!
//! Split out of the main `files_tests.rs` so each companion file stays
//! within rustqual's SRP_MODULE cluster budget. The filter tests here
//! share the same fixture shape (small tempdir with a supported + an
//! unsupported file); the non-filter basics live next door.

use super::{list_files, FilesFilter};
use tempfile::TempDir;

#[test]
fn list_files_skipped_only() {
    let tmp = TempDir::new().unwrap();

    std::fs::write(tmp.path().join("main.rs"), "").unwrap(); // supported
    std::fs::write(tmp.path().join("data.xyz"), "").unwrap(); // unsupported

    let filter = FilesFilter {
        skipped_only: true,
        ..Default::default()
    };
    let result = list_files(tmp.path(), filter).unwrap();
    // Only unsupported files
    for f in &result.results {
        assert!(!f.supported);
    }
}

#[test]
fn list_files_indexed_only() {
    let tmp = TempDir::new().unwrap();

    std::fs::write(tmp.path().join("main.rs"), "").unwrap(); // supported
    std::fs::write(tmp.path().join("data.xyz"), "").unwrap(); // unsupported

    let filter = FilesFilter {
        indexed_only: true,
        ..Default::default()
    };
    let result = list_files(tmp.path(), filter).unwrap();
    // Only supported files
    for f in &result.results {
        assert!(f.supported);
    }
}
