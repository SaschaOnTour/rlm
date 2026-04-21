//! Detection tests for `verify.rs`.
//!
//! Moved into a companion file so the corruption-simulation helpers
//! (orphan chunks, missing-file index entries) can use `rusqlite::params`
//! without triggering the `no_rusqlite_outside_infrastructure` rule on
//! production code. Wired back in via
//! `#[cfg(test)] #[path = "verify_tests.rs"] mod tests;`.
//!
//! `fix_integrity` / orphan-repair tests live in the sibling
//! `verify_fix_tests.rs`.

use super::super::fixtures::setup_test_db_and_dir;
use super::verify_index;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 12;

#[test]
fn verify_clean_index() {
    let (db, tmp) = setup_test_db_and_dir();

    // Create file on disk and index it
    let file_path = tmp.path().join("test.rs");
    std::fs::write(&file_path, "fn main() {}").unwrap();

    let file = FileRecord::new(
        "test.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let chunk = Chunk {
        id: 0,
        file_id,
        start_line: 1,
        end_line: 1,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "main".into(),
        parent: None,
        signature: None,
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn main() {}".into(),
    };
    db.insert_chunk(&chunk).unwrap();

    let report = verify_index(&db, tmp.path()).unwrap();
    assert!(report.is_ok());
    assert!(report.sqlite_ok);
    assert_eq!(report.orphan_chunks, 0);
    assert_eq!(report.orphan_refs, 0);
    assert_eq!(report.missing_files, 0);
}

#[test]
fn verify_detects_missing_file() {
    let (db, tmp) = setup_test_db_and_dir();

    // Index a file but don't create it on disk
    let file = FileRecord::new(
        "missing.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    db.upsert_file(&file).unwrap();

    let report = verify_index(&db, tmp.path()).unwrap();
    assert!(!report.is_ok());
    assert_eq!(report.missing_files, 1);
    assert!(report
        .missing_file_paths
        .contains(&"missing.rs".to_string()));
}
