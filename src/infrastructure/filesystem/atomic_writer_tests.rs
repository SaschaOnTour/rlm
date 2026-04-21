//! Tests for `atomic_writer.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "atomic_writer_tests.rs"] mod tests;`.

use super::{write_atomic, TEMP_PREFIX};
use tempfile::TempDir;

#[test]
fn write_creates_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("out.txt");
    write_atomic(&path, b"hello").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"hello");
}

#[test]
fn write_overwrites_existing_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("out.txt");
    std::fs::write(&path, b"before").unwrap();
    write_atomic(&path, b"after").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"after");
}

#[test]
fn write_creates_missing_parent() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("nested/dir/out.txt");
    write_atomic(&path, b"x").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"x");
}

#[test]
fn write_leaves_no_temp_files_behind() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("out.txt");
    write_atomic(&path, b"data").unwrap();
    let leftover_count = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(TEMP_PREFIX))
        .count();
    assert_eq!(leftover_count, 0, "no .rlm_tmp_* files should remain");
}
