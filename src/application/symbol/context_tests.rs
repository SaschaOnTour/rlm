//! Basic tests for `context.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "context_tests.rs"] mod tests;`.
//!
//! Graph-related tests (multi-definition, ref-kind filtering) live in
//! the sibling `context_graph_tests.rs`.

use super::super::fixtures::setup_test_db;
use super::build_context;
use crate::domain::chunk::{Chunk, ChunkKind, Reference};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 200;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TARGET_START_LINE: u32 = 10;
const TARGET_END_LINE: u32 = 20;
const TARGET_START_BYTE: u32 = 100;
const TARGET_END_BYTE: u32 = 300;
const TEST_END_LINE_SHORT: u32 = 3;
const TEST_REF_COL: u32 = 5;
const VALIDATE_REF_LINE: u32 = 12;
const TRANSFORM_REF_LINE: u32 = 13;

#[test]
fn test_context_empty_symbol() {
    let db = setup_test_db();
    let result = build_context(&db, "nonexistent").unwrap();

    assert_eq!(result.symbol, "nonexistent");
    assert!(result.body.is_empty());
    assert!(result.signatures.is_empty());
    assert_eq!(result.caller_count, 0);
    assert!(result.callee_names.is_empty());
}

#[test]
fn test_context_basic() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/lib.rs".to_string(),
        "abc123".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let target = Chunk {
        start_line: TARGET_START_LINE,
        end_line: TARGET_END_LINE,
        start_byte: TARGET_START_BYTE,
        end_byte: TARGET_END_BYTE,
        kind: ChunkKind::Function,
        ident: "process_data".to_string(),
        signature: Some("fn process_data(input: &str) -> Result<String>".to_string()),
        visibility: Some("pub".to_string()),
        doc_comment: Some("Process the input data".to_string()),
        content: "fn process_data(input: &str) -> Result<String> {\n    validate(input)?;\n    transform(input)\n}".to_string(),
        ..Chunk::stub(file_id)
    };
    let target_id = db.insert_chunk(&target).unwrap();

    let caller = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "main".to_string(),
        signature: Some("fn main()".to_string()),
        content: "fn main() { process_data(\"test\"); }".to_string(),
        ..Chunk::stub(file_id)
    };
    let caller_id = db.insert_chunk(&caller).unwrap();

    let caller_ref = Reference {
        target_ident: "process_data".to_string(),
        line: TEST_END_LINE_SHORT,
        col: TEST_REF_COL,
        ..Reference::stub(caller_id)
    };
    db.insert_ref(&caller_ref).unwrap();

    let ref1 = Reference {
        target_ident: "validate".to_string(),
        line: VALIDATE_REF_LINE,
        col: TEST_REF_COL,
        ..Reference::stub(target_id)
    };
    db.insert_ref(&ref1).unwrap();

    let ref2 = Reference {
        target_ident: "transform".to_string(),
        line: TRANSFORM_REF_LINE,
        col: TEST_REF_COL,
        ..Reference::stub(target_id)
    };
    db.insert_ref(&ref2).unwrap();

    let result = build_context(&db, "process_data").unwrap();

    assert_eq!(result.symbol, "process_data");
    assert_eq!(result.body.len(), 1);
    assert!(result.body[0].contains("process_data"));
    assert_eq!(result.signatures.len(), 1);
    assert!(result.signatures[0].contains("Result<String>"));
    assert_eq!(result.caller_count, 1);
    assert_eq!(result.callee_names.len(), 2);
    assert!(result.callee_names.contains(&"validate".to_string()));
    assert!(result.callee_names.contains(&"transform".to_string()));
    assert!(
        result.tokens.output > 0,
        "token estimate should be non-zero for non-empty result"
    );
}
