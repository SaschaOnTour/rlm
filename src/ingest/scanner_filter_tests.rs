//! Filter / skip-reason tests for `scanner.rs`.
//!
//! Split out of `scanner_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). The language-mapping and
//! discovery happy-path tests stay in `scanner_tests.rs`; this file
//! covers the skip semantics (.gitignore, `.rlm/`, size cap).

use super::{Scanner, SkipReason, BYTES_PER_MB};
use std::fs;
use tempfile::TempDir;

/// File size limit in bytes for the large-file-skip tests.
const TEST_MAX_FILE_SIZE_BYTES: u64 = 1000;
/// Content length used to create a file larger than `TEST_MAX_FILE_SIZE_BYTES`.
const LARGE_FILE_CONTENT_LENGTH: usize = 2000;
/// Expected max_file_size_bytes when constructing with 10 MB.
const TEST_MAX_FILE_SIZE_MB: u32 = 10;
const TEN_MB_IN_BYTES: u64 = TEST_MAX_FILE_SIZE_MB as u64 * BYTES_PER_MB;

#[test]
fn scan_all_respects_gitignore() {
    let tmp = TempDir::new().unwrap();
    // Create a minimal .git directory so ignore crate respects .gitignore
    fs::create_dir(tmp.path().join(".git")).unwrap();
    fs::write(tmp.path().join(".gitignore"), "ignored.rs").unwrap();
    fs::write(tmp.path().join("main.rs"), "").unwrap();
    fs::write(tmp.path().join("ignored.rs"), "").unwrap();

    let scanner = Scanner::new(tmp.path());
    let files = scanner.scan_all().unwrap();

    // Should not include ignored.rs (respects .gitignore)
    assert!(!files.iter().any(|f| f.path.contains("ignored.rs")));
    // But should include main.rs
    assert!(files.iter().any(|f| f.path.contains("main.rs")));
}

#[test]
fn scan_all_skips_rlm_dir() {
    let tmp = TempDir::new().unwrap();
    let rlm_dir = tmp.path().join(".rlm");
    fs::create_dir_all(&rlm_dir).unwrap();
    fs::write(rlm_dir.join("index.db"), "binary").unwrap();
    fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();

    let scanner = Scanner::new(tmp.path());
    let files = scanner.scan_all().unwrap();

    assert_eq!(files.len(), 1);
    assert!(files[0].path.contains("main.rs"));
}

#[test]
fn scanner_skips_large_files() {
    let tmp = TempDir::new().unwrap();
    // Create a file larger than TEST_MAX_FILE_SIZE_BYTES
    let large_content = "x".repeat(LARGE_FILE_CONTENT_LENGTH);
    fs::write(tmp.path().join("large.rs"), &large_content).unwrap();
    fs::write(tmp.path().join("small.rs"), "fn main() {}").unwrap();

    let scanner = Scanner {
        root: tmp.path().to_path_buf(),
        max_file_size_bytes: TEST_MAX_FILE_SIZE_BYTES,
    };
    let files = scanner.scan().unwrap();

    // Only the small file should be included
    assert_eq!(files.len(), 1);
    assert!(files[0].relative_path.contains("small.rs"));
}

#[test]
fn scan_all_reports_large_files() {
    let tmp = TempDir::new().unwrap();
    let large_content = "x".repeat(LARGE_FILE_CONTENT_LENGTH);
    fs::write(tmp.path().join("large.rs"), &large_content).unwrap();
    fs::write(tmp.path().join("small.rs"), "fn main() {}").unwrap();

    let scanner = Scanner {
        root: tmp.path().to_path_buf(),
        max_file_size_bytes: TEST_MAX_FILE_SIZE_BYTES,
    };
    let files = scanner.scan_all().unwrap();

    // Both files should be listed
    assert_eq!(files.len(), 2);

    let large = files.iter().find(|f| f.path.contains("large")).unwrap();
    assert!(!large.supported);
    assert_eq!(large.reason, Some(SkipReason::TooLarge));

    let small = files.iter().find(|f| f.path.contains("small")).unwrap();
    assert!(small.supported);
    assert!(small.reason.is_none());
}

#[test]
fn with_max_file_size_constructor() {
    let tmp = TempDir::new().unwrap();
    let scanner = Scanner::with_max_file_size(tmp.path(), TEST_MAX_FILE_SIZE_MB);
    assert_eq!(scanner.max_file_size_bytes, TEN_MB_IN_BYTES);
}
