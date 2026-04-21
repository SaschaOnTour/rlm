//! RefKind-centric impact tests for `impact.rs`.
//!
//! Split out of `impact_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). The base tests
//! (`file_count_*`, `analyze_impact` basic flow) stay in the original;
//! this file covers the type-use / cross-file assertions.

use super::super::fixtures::setup_test_db;
use super::analyze_impact;
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES_MEDIUM: u64 = 100;
const TEST_SMALL_FILE_BYTES: u64 = 50;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_END_LINE_SHORT: u32 = 3;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TEST_END_BYTE_SMALL: u32 = 30;
const TEST_REF_COL: u32 = 5;
const TYPE_USER_START_LINE: u32 = 10;
const TYPE_USER_END_LINE: u32 = 15;
const TYPE_USER_START_BYTE: u32 = 100;
const TYPE_USER_END_BYTE: u32 = 180;
const TYPE_REF_COL: u32 = 18;
const CROSS_FILE_CALLER_START_LINE: u32 = 5;
const CROSS_FILE_CALLER_END_LINE: u32 = 10;
const CROSS_FILE_CALLER_END_BYTE: u32 = 60;
const CROSS_FILE_REF_LINE: u32 = 7;

#[test]
fn test_impact_includes_ref_kind() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/types.rs".to_string(),
        "def456".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES_MEDIUM,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let type_def = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Struct,
        ident: "MyStruct".to_string(),
        signature: Some("struct MyStruct".to_string()),
        visibility: Some("pub".to_string()),
        content: "struct MyStruct { }".to_string(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&type_def).unwrap();

    let func = Chunk {
        start_line: TYPE_USER_START_LINE,
        end_line: TYPE_USER_END_LINE,
        start_byte: TYPE_USER_START_BYTE,
        end_byte: TYPE_USER_END_BYTE,
        kind: ChunkKind::Function,
        ident: "use_type".to_string(),
        signature: Some("fn use_type(x: MyStruct)".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn use_type(x: MyStruct) { }".to_string(),
        ..Chunk::stub(file_id)
    };
    let func_id = db.insert_chunk(&func).unwrap();

    let type_ref = Reference {
        target_ident: "MyStruct".to_string(),
        ref_kind: RefKind::TypeUse,
        line: TYPE_USER_START_LINE,
        col: TYPE_REF_COL,
        ..Reference::stub(func_id)
    };
    db.insert_ref(&type_ref).unwrap();

    let result = analyze_impact(&db, "MyStruct").unwrap();

    assert_eq!(result.count, 1);
    assert_eq!(result.impacted[0].ref_kind, "type_use");
    assert_eq!(result.impacted[0].in_symbol, "use_type");
}

#[test]
fn test_impact_across_files() {
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

    let target = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "shared_fn".to_string(),
        signature: Some("fn shared_fn()".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn shared_fn() { }".to_string(),
        ..Chunk::stub(file1_id)
    };
    db.insert_chunk(&target).unwrap();

    let caller = Chunk {
        start_line: CROSS_FILE_CALLER_START_LINE,
        end_line: CROSS_FILE_CALLER_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: CROSS_FILE_CALLER_END_BYTE,
        kind: ChunkKind::Function,
        ident: "consumer".to_string(),
        signature: Some("fn consumer()".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn consumer() { shared_fn(); }".to_string(),
        ..Chunk::stub(file2_id)
    };
    let caller_id = db.insert_chunk(&caller).unwrap();

    let ref_to_target = Reference {
        target_ident: "shared_fn".to_string(),
        line: CROSS_FILE_REF_LINE,
        col: TEST_REF_COL,
        ..Reference::stub(caller_id)
    };
    db.insert_ref(&ref_to_target).unwrap();

    let result = analyze_impact(&db, "shared_fn").unwrap();

    assert_eq!(result.count, 1);
    assert_eq!(result.impacted[0].file, "src/b.rs");
    assert_eq!(result.impacted[0].in_symbol, "consumer");
}
