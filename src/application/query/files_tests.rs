//! Basic-path tests for `list_files`: total counts and prefix filter.
//!
//! Filter-semantics tests (`skipped_only`, `indexed_only`) live in the
//! sibling `files_filter_tests.rs` so each companion stays small.

use super::{list_files, FilesFilter};
use tempfile::TempDir;

#[test]
fn list_files_basic() {
    let tmp = TempDir::new().unwrap();

    // Create some files
    std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(tmp.path().join("lib.rs"), "// lib").unwrap();
    std::fs::write(tmp.path().join("README.md"), "# README").unwrap();

    let result = list_files(tmp.path(), FilesFilter::default()).unwrap();
    assert_eq!(result.summary.total, 3);
    assert!(result.summary.indexed > 0);
}

#[test]
fn list_files_with_path_filter() {
    let tmp = TempDir::new().unwrap();

    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    std::fs::write(tmp.path().join("src/main.rs"), "").unwrap();
    std::fs::write(tmp.path().join("tests/test.rs"), "").unwrap();

    let filter = FilesFilter {
        path_prefix: Some("src".into()),
        ..Default::default()
    };
    let result = list_files(tmp.path(), filter).unwrap();
    assert_eq!(result.summary.total, 1);
    assert!(result.results[0].path.starts_with("src"));
}
