//! Tests for `refs.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "refs_tests.rs"] mod tests;`.

use super::{get_refs, Database};
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TEST_REF_COL: u32 = 14;

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn get_refs_basic() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let chunk = Chunk {
        id: 0,
        file_id,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "caller".into(),
        parent: None,
        signature: None,
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn caller() { foo(); }".into(),
    };
    let chunk_id = db.insert_chunk(&chunk).unwrap();

    let reference = Reference {
        id: 0,
        chunk_id,
        target_ident: "foo".into(),
        ref_kind: RefKind::Call,
        line: 1,
        col: TEST_REF_COL,
    };
    db.insert_ref(&reference).unwrap();

    let result = get_refs(&db, "foo").unwrap();
    assert_eq!(result.symbol, "foo");
    assert_eq!(result.count, 1);
    assert_eq!(result.refs[0].kind, "call");
    assert_eq!(result.refs[0].line, 1);
    assert_eq!(result.refs[0].col, TEST_REF_COL);
}

#[test]
fn get_refs_empty() {
    let db = test_db();
    let result = get_refs(&db, "nonexistent").unwrap();
    assert_eq!(result.count, 0);
    assert!(result.refs.is_empty());
}
