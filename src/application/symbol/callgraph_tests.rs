//! Basic tests for `callgraph.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "callgraph_tests.rs"] mod tests;`.
//!
//! Reference-kind / caller-edge tests live in the sibling
//! `callgraph_refs_tests.rs`.

use super::super::fixtures::setup_test_db;
use super::build_callgraph;
use crate::domain::chunk::{Chunk, ChunkKind, Reference};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TARGET_START_LINE: u32 = 10;
const TARGET_END_LINE: u32 = 15;
const TARGET_START_BYTE: u32 = 100;
const TARGET_END_BYTE: u32 = 150;
const TEST_REF_LINE_A: u32 = 3;
const TEST_REF_LINE_B: u32 = 12;
const TEST_REF_COL: u32 = 5;

#[test]
fn test_callgraph_empty_symbol() {
    let db = setup_test_db();
    let result = build_callgraph(&db, "nonexistent").unwrap();

    assert_eq!(result.symbol, "nonexistent");
    assert!(result.callers.is_empty());
    assert!(result.callees.is_empty());
}

#[test]
fn test_callgraph_basic() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/lib.rs".to_string(),
        "abc123".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let caller = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "caller_fn".to_string(),
        signature: Some("fn caller_fn()".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn caller_fn() { target_fn(); }".to_string(),
        ..Chunk::stub(file_id)
    };
    let caller_id = db.insert_chunk(&caller).unwrap();

    let target = Chunk {
        start_line: TARGET_START_LINE,
        end_line: TARGET_END_LINE,
        start_byte: TARGET_START_BYTE,
        end_byte: TARGET_END_BYTE,
        kind: ChunkKind::Function,
        ident: "target_fn".to_string(),
        signature: Some("fn target_fn()".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn target_fn() { helper(); }".to_string(),
        ..Chunk::stub(file_id)
    };
    let target_id = db.insert_chunk(&target).unwrap();

    let ref_to_target = Reference {
        target_ident: "target_fn".to_string(),
        line: TEST_REF_LINE_A,
        col: TEST_REF_COL,
        ..Reference::stub(caller_id)
    };
    db.insert_ref(&ref_to_target).unwrap();

    let ref_to_helper = Reference {
        target_ident: "helper".to_string(),
        line: TEST_REF_LINE_B,
        col: TEST_REF_COL,
        ..Reference::stub(target_id)
    };
    db.insert_ref(&ref_to_helper).unwrap();

    let result = build_callgraph(&db, "target_fn").unwrap();

    assert_eq!(result.symbol, "target_fn");
    assert_eq!(result.callers, vec!["caller_fn"]);
    assert_eq!(result.callees, vec!["helper"]);
}
