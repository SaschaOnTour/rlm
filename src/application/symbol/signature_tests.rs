//! Tests for `signature.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "signature_tests.rs"] mod tests;`.

use super::{get_signature, Database};
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_END_LINE_SHORT: u32 = 3;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TEST_END_BYTE_SMALL: u32 = 30;
const TEST_REF_LINE: u32 = 10;
const TEST_REF_COL: u32 = 4;

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn get_signature_basic() {
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
        ident: "foo".into(),
        parent: None,
        signature: Some("fn foo(x: i32) -> String".into()),
        visibility: Some("pub".into()),
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "pub fn foo(x: i32) -> String { }".into(),
    };
    let chunk_id = db.insert_chunk(&chunk).unwrap();

    // Add some refs
    let reference = Reference {
        id: 0,
        chunk_id,
        target_ident: "foo".into(),
        ref_kind: RefKind::Call,
        line: TEST_REF_LINE,
        col: TEST_REF_COL,
    };
    db.insert_ref(&reference).unwrap();

    let result = get_signature(&db, "foo").unwrap();
    assert_eq!(result.symbol, "foo");
    assert_eq!(result.signatures, vec!["fn foo(x: i32) -> String"]);
    assert_eq!(result.ref_count, 1);
}

#[test]
fn get_signature_no_signature() {
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
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Module,
        ident: "mymod".into(),
        parent: None,
        signature: None, // Modules may not have signatures
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "mod mymod {}".into(),
    };
    db.insert_chunk(&chunk).unwrap();

    let result = get_signature(&db, "mymod").unwrap();
    assert!(result.signatures.is_empty());
}
