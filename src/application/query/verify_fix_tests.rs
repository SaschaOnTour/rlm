//! Fix-integrity tests for `verify.rs`.
//!
//! Split out of `verify_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Detection-only tests
//! (`verify_index` happy path, missing files) stay in `verify_tests.rs`;
//! this file covers `fix_integrity` and the orphan-repair paths that
//! rely on the `rusqlite::params` FK-bypass corruption simulation.

use super::super::fixtures::setup_test_db_and_dir;
use super::{fix_integrity, verify_index};
use crate::db::Database;
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;
use rusqlite::params;
use tempfile::TempDir;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE_SMALL: u32 = 10;

#[test]
fn fix_removes_missing_files() {
    let (db, tmp) = setup_test_db_and_dir();

    // Index a file that doesn't exist on disk
    let file = FileRecord::new(
        "missing.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    db.upsert_file(&file).unwrap();

    let report = verify_index(&db, tmp.path()).unwrap();
    let fix_result = fix_integrity(&db, &report).unwrap();

    assert!(fix_result.fixed);
    assert_eq!(fix_result.missing_files_removed, 1);

    // Verify it's actually removed
    let files = db.get_all_files().unwrap();
    assert!(files.is_empty());
}

#[test]
fn verify_and_fix_orphan_chunks() {
    let db = Database::open_in_memory().unwrap();
    let tmp = TempDir::new().unwrap();

    let file1 = FileRecord::new(
        "test.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let _file1_id = db.upsert_file(&file1).unwrap();

    let file2 = FileRecord::new(
        "other.rs".into(),
        "hash2".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file2_id = db.upsert_file(&file2).unwrap();

    // Create a chunk for file2
    let chunk = Chunk {
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "orphan_soon".into(),
        content: "fn orphan()".into(),
        ..Chunk::stub(file2_id)
    };
    db.insert_chunk(&chunk).unwrap();

    // Create both files on disk
    std::fs::write(tmp.path().join("test.rs"), "").unwrap();
    std::fs::write(tmp.path().join("other.rs"), "").unwrap();

    // Delete file2 directly (bypassing cascade to create orphan)
    db.conn().execute("PRAGMA foreign_keys = OFF;", []).unwrap();
    db.conn()
        .execute("DELETE FROM files WHERE id = ?1", params![file2_id])
        .unwrap();
    db.conn().execute("PRAGMA foreign_keys = ON;", []).unwrap();

    // Now we have an orphan chunk
    let report = verify_index(&db, tmp.path()).unwrap();
    assert_eq!(report.orphan_chunks, 1);

    let fix_result = fix_integrity(&db, &report).unwrap();
    assert_eq!(fix_result.orphan_chunks_deleted, 1);

    let all_chunks = db.get_all_chunks().unwrap();
    assert!(all_chunks.is_empty());
}

#[test]
fn verify_and_fix_orphan_refs() {
    let db = Database::open_in_memory().unwrap();
    let tmp = TempDir::new().unwrap();

    // Create a file and chunk
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
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "test".into(),
        parent: None,
        signature: None,
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn test()".into(),
    };
    let chunk_id = db.insert_chunk(&chunk).unwrap();

    // Create a valid ref
    let valid_ref = Reference {
        id: 0,
        chunk_id,
        target_ident: "foo".into(),
        ref_kind: RefKind::Call,
        line: 1,
        col: 1,
    };
    db.insert_ref(&valid_ref).unwrap();

    // Create the file on disk
    std::fs::write(tmp.path().join("test.rs"), "").unwrap();

    // Delete the chunk directly (bypassing cascade to create orphan ref)
    db.conn().execute("PRAGMA foreign_keys = OFF;", []).unwrap();
    db.conn()
        .execute("DELETE FROM chunks WHERE id = ?1", params![chunk_id])
        .unwrap();
    db.conn().execute("PRAGMA foreign_keys = ON;", []).unwrap();

    // Now we have an orphan ref
    let report = verify_index(&db, tmp.path()).unwrap();
    assert_eq!(report.orphan_refs, 1);

    let fix_result = fix_integrity(&db, &report).unwrap();
    assert_eq!(fix_result.orphan_refs_deleted, 1);
}
