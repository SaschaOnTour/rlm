//! Tests for `type_info.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "type_info_tests.rs"] mod tests;`.

use super::{get_type_info, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_END_LINE_SHORT: u32 = 3;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TEST_END_BYTE_SMALL: u32 = 30;

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn get_type_info_basic() {
    let db = test_db();

    // Insert a file and chunk
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
        kind: ChunkKind::Struct,
        ident: "MyStruct".into(),
        parent: None,
        signature: Some("struct MyStruct".into()),
        visibility: Some("pub".into()),
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "pub struct MyStruct {\n    field: i32,\n}".into(),
    };
    db.insert_chunk(&chunk).unwrap();

    let result = get_type_info(&db, "MyStruct").unwrap();
    assert_eq!(result.symbol, "MyStruct");
    assert_eq!(result.kind, "struct");
    assert_eq!(result.signature, Some("struct MyStruct".into()));
    assert_eq!(result.file, "src/lib.rs");
}

#[test]
fn get_type_info_prioritizes_src() {
    let db = test_db();

    let fixture_file = FileRecord::new(
        "fixtures/test.rs".into(),
        "h1".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let fixture_id = db.upsert_file(&fixture_file).unwrap();

    let src_file = FileRecord::new(
        "src/lib.rs".into(),
        "h2".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let src_id = db.upsert_file(&src_file).unwrap();

    // Same symbol in both files
    let fixture_chunk = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "foo".into(),
        signature: Some("fn foo() [fixture]".into()),
        content: "fn foo() { fixture }".into(),
        ..Chunk::stub(fixture_id)
    };
    db.insert_chunk(&fixture_chunk).unwrap();

    let src_chunk = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "foo".into(),
        signature: Some("fn foo() [src]".into()),
        content: "fn foo() { src }".into(),
        ..Chunk::stub(src_id)
    };
    db.insert_chunk(&src_chunk).unwrap();

    let result = get_type_info(&db, "foo").unwrap();
    // Should prioritize src/ over fixtures/
    assert_eq!(result.file, "src/lib.rs");
    assert_eq!(result.signature, Some("fn foo() [src]".into()));
}

#[test]
fn get_type_info_symbol_not_found() {
    let db = test_db();
    let result = get_type_info(&db, "NonExistent");
    assert!(result.is_err());
}
