//! File / ref / stats / cascade tests for `db::queries::mod`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "mod_tests.rs"] mod tests;`.
//!
//! Chunk-specific tests (insert, by-id, FTS, by-ident) live in the
//! sibling `mod_chunk_tests.rs`.

use super::test_fixtures::{sample_chunk, sample_file, test_db};
use crate::domain::chunk::{RefKind, Reference};
use crate::domain::file::FileRecord;

const UPDATED_FILE_SIZE: u64 = 2048;
const REF_COL: u32 = 4;

#[test]
fn upsert_file_and_retrieve() {
    let db = test_db();
    let f = sample_file();
    let id = db.upsert_file(&f).unwrap();
    assert!(id > 0);
    let got = db.get_file_by_path("src/main.rs").unwrap().unwrap();
    assert_eq!(got.hash, "abc123");
}

#[test]
fn upsert_file_updates_existing() {
    let db = test_db();
    let f = sample_file();
    db.upsert_file(&f).unwrap();
    let f2 = FileRecord::new(
        "src/main.rs".into(),
        "def456".into(),
        "rust".into(),
        UPDATED_FILE_SIZE,
    );
    db.upsert_file(&f2).unwrap();
    let got = db.get_file_by_path("src/main.rs").unwrap().unwrap();
    assert_eq!(got.hash, "def456");
}

#[test]
fn insert_ref_and_find() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    let c = sample_chunk(fid);
    let cid = db.insert_chunk(&c).unwrap();
    let r = Reference {
        id: 0,
        chunk_id: cid,
        target_ident: "println".into(),
        ref_kind: RefKind::Call,
        line: 2,
        col: REF_COL,
    };
    db.insert_ref(&r).unwrap();
    let refs = db.get_refs_to("println").unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].ref_kind, RefKind::Call);
}

#[test]
fn stats_returns_counts() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    let c = sample_chunk(fid);
    db.insert_chunk(&c).unwrap();
    let stats = db.stats().unwrap();
    assert_eq!(stats.file_count, 1);
    assert_eq!(stats.chunk_count, 1);
    assert_eq!(stats.languages.len(), 1);
}

#[test]
fn delete_file_cascades() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    let c = sample_chunk(fid);
    db.insert_chunk(&c).unwrap();
    db.delete_file(fid).unwrap();
    let files = db.get_all_files().unwrap();
    assert!(files.is_empty());
    let chunks = db.get_all_chunks().unwrap();
    assert!(chunks.is_empty());
}
