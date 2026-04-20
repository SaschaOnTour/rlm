//! Index-pipeline tests for `mod.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "mod_tests.rs"] mod tests;`.
//!
//! Reindex / preview tests live in the sibling `mod_reindex_tests.rs`.

use super::{run_index, Config};
use std::fs;
use tempfile::TempDir;

/// Non-UTF-8 byte sequence used to test binary file rejection.
const NON_UTF8_BYTES: [u8; 4] = [0xFF, 0xFE, 0x00, 0x01];

#[test]
fn index_rust_project() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join("main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n\nfn helper() -> i32 {\n    42\n}\n",
    )
    .unwrap();

    let config = Config::new(tmp.path());
    let result = run_index(&config, None).unwrap();

    assert!(result.files_indexed > 0);
    assert!(result.chunks_created > 0);
    assert!(config.index_exists());
}

#[test]
fn incremental_index_skips_unchanged() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();

    let config = Config::new(tmp.path());

    // First index
    let r1 = run_index(&config, None).unwrap();
    assert!(r1.files_indexed > 0);

    // Second index (no changes)
    let r2 = run_index(&config, None).unwrap();
    assert_eq!(r2.files_indexed, 0);
    assert!(r2.files_skipped > 0);
}

#[test]
fn incremental_index_reindexes_changed() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    let file_path = src_dir.join("main.rs");
    fs::write(&file_path, "fn main() {}").unwrap();

    let config = Config::new(tmp.path());
    run_index(&config, None).unwrap();

    // Modify file
    fs::write(&file_path, "fn main() { println!(\"changed\"); }").unwrap();

    let r2 = run_index(&config, None).unwrap();
    assert!(r2.files_indexed > 0);
}

#[test]
fn incremental_index_removes_deleted_files() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    // Create two files
    fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();
    fs::write(src_dir.join("helper.rs"), "fn helper() {}").unwrap();

    let config = Config::new(tmp.path());
    let r1 = run_index(&config, None).unwrap();
    assert_eq!(r1.files_indexed, 2);

    // Delete one file
    fs::remove_file(src_dir.join("helper.rs")).unwrap();

    let r2 = run_index(&config, None).unwrap();
    assert_eq!(r2.deleted_from_index, 1);
    assert_eq!(r2.skipped_unchanged, 1); // main.rs unchanged

    // Verify only main.rs remains in the database
    let db = crate::db::Database::open(&config.db_path).unwrap();
    let files = db.get_all_files().unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0].path.contains("main.rs"));
}

#[test]
fn index_result_categorizes_skips() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    // Create a valid Rust file
    fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();

    // Create a binary file (non-UTF8)
    fs::write(src_dir.join("binary.rs"), NON_UTF8_BYTES).unwrap();

    let config = Config::new(tmp.path());
    let result = run_index(&config, None).unwrap();

    assert_eq!(result.files_indexed, 1);
    assert_eq!(result.skipped_non_utf8, 1);
    assert_eq!(result.files_skipped, 1);
}
