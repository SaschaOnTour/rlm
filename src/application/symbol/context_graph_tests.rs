//! Graph-related context tests for `context.rs`.
//!
//! Split out of `context_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). The basic empty / happy
//! path tests stay in `context_tests.rs`; this file covers multi-file
//! definitions and callee graph / ref-kind filtering.

use super::super::fixtures::setup_test_db;
use super::build_context;
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;

const TEST_SMALL_FILE_BYTES: u64 = 50;
const TEST_FILE_BYTES_MEDIUM: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE_SMALL: u32 = 30;
const TEST_END_BYTE_MEDIUM: u32 = 40;
const TEST_END_BYTE_LARGE: u32 = 80;
const TEST_END_LINE_SHORT: u32 = 3;
const TEST_REF_COL: u32 = 5;
const TYPE_REF_COL: u32 = 15;

#[test]
fn test_context_multiple_definitions() {
    let db = setup_test_db();

    let file1 = FileRecord::new(
        "src/a.rs".to_string(),
        "aaa".to_string(),
        "rust".to_string(),
        TEST_SMALL_FILE_BYTES,
    );
    let file1_id = db.upsert_file(&file1).unwrap();

    let file2 = FileRecord::new(
        "src/b.rs".to_string(),
        "bbb".to_string(),
        "rust".to_string(),
        TEST_SMALL_FILE_BYTES,
    );
    let file2_id = db.upsert_file(&file2).unwrap();

    let chunk1 = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "new".to_string(),
        parent: Some("StructA".to_string()),
        signature: Some("fn new() -> Self".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn new() -> Self { Self {} }".to_string(),
        ..Chunk::stub(file1_id)
    };
    db.insert_chunk(&chunk1).unwrap();

    let chunk2 = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_MEDIUM,
        kind: ChunkKind::Function,
        ident: "new".to_string(),
        parent: Some("StructB".to_string()),
        signature: Some("fn new(val: i32) -> Self".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn new(val: i32) -> Self { Self { val } }".to_string(),
        ..Chunk::stub(file2_id)
    };
    db.insert_chunk(&chunk2).unwrap();

    let result = build_context(&db, "new").unwrap();

    assert_eq!(result.symbol, "new");
    assert_eq!(result.body.len(), 2);
    assert_eq!(result.signatures.len(), 2);
    assert_eq!(result.file_count, 2); // defined in 2 distinct files
}

#[test]
fn test_context_filters_non_call_refs() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/lib.rs".to_string(),
        "xyz".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES_MEDIUM,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let func = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_LARGE,
        kind: ChunkKind::Function,
        ident: "handler".to_string(),
        signature: Some("fn handler(req: Request)".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn handler(req: Request) { process(); }".to_string(),
        ..Chunk::stub(file_id)
    };
    let func_id = db.insert_chunk(&func).unwrap();

    let call_ref = Reference {
        target_ident: "process".to_string(),
        line: 2,
        col: TEST_REF_COL,
        ..Reference::stub(func_id)
    };
    db.insert_ref(&call_ref).unwrap();

    let import_ref = Reference {
        target_ident: "Request".to_string(),
        ref_kind: RefKind::Import,
        line: 1,
        col: TYPE_REF_COL,
        ..Reference::stub(func_id)
    };
    db.insert_ref(&import_ref).unwrap();

    let result = build_context(&db, "handler").unwrap();

    assert_eq!(result.callee_names.len(), 1);
    assert!(result.callee_names.contains(&"process".to_string()));
    assert!(!result.callee_names.contains(&"Request".to_string()));
}
