//! Reference-kind / caller-edge tests for `callgraph.rs`.
//!
//! Split out of `callgraph_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Basic empty-symbol / happy
//! path cases stay in `callgraph_tests.rs`; this file covers the
//! no-callers case and the non-call RefKind filtering.

use super::super::fixtures::setup_test_db;
use super::build_callgraph;
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_SMALL_FILE_BYTES: u64 = 50;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TEST_END_BYTE_LARGE: u32 = 80;
const TEST_REF_COL: u32 = 5;
const TYPE_REF_COL: u32 = 15;

#[test]
fn test_callgraph_no_callers() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/main.rs".to_string(),
        "def456".to_string(),
        "rust".to_string(),
        TEST_SMALL_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let main_fn = Chunk {
        id: 0,
        file_id,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "main".to_string(),
        parent: None,
        signature: Some("fn main()".to_string()),
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn main() { println!(); }".to_string(),
    };
    let main_id = db.insert_chunk(&main_fn).unwrap();

    let ref_to_println = Reference {
        target_ident: "println".to_string(),
        line: 2,
        col: TEST_REF_COL,
        ..Reference::stub(main_id)
    };
    db.insert_ref(&ref_to_println).unwrap();

    let result = build_callgraph(&db, "main").unwrap();

    assert_eq!(result.symbol, "main");
    assert!(result.callers.is_empty());
    assert_eq!(result.callees, vec!["println"]);
}

#[test]
fn test_callgraph_filters_non_call_refs() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/types.rs".to_string(),
        "ghi789".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let func = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_LARGE,
        kind: ChunkKind::Function,
        ident: "process".to_string(),
        signature: Some("fn process(x: MyType)".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn process(x: MyType) { helper(); }".to_string(),
        ..Chunk::stub(file_id)
    };
    let func_id = db.insert_chunk(&func).unwrap();

    let call_ref = Reference {
        target_ident: "helper".to_string(),
        line: 2,
        col: TEST_REF_COL,
        ..Reference::stub(func_id)
    };
    db.insert_ref(&call_ref).unwrap();

    let type_ref = Reference {
        target_ident: "MyType".to_string(),
        ref_kind: RefKind::TypeUse,
        line: 1,
        col: TYPE_REF_COL,
        ..Reference::stub(func_id)
    };
    db.insert_ref(&type_ref).unwrap();

    let result = build_callgraph(&db, "process").unwrap();

    assert_eq!(result.callees.len(), 1);
    assert!(result.callees.contains(&"helper".to_string()));
    assert!(!result.callees.contains(&"MyType".to_string()));
}
