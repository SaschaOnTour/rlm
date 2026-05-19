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
    assert_eq!(result.callers.len(), 1);
    assert_eq!(result.callers[0].ident, "caller_fn");
    assert!(result.callers[0].parent.is_none());
    assert_eq!(result.callees.len(), 1);
    assert_eq!(result.callees[0].ident, "helper");
}

// ─── Slice 0.8: parent on callers and callees ─────────────────────────

#[test]
fn callgraph_carries_parent_on_method_callers() {
    let db = setup_test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();

    // target free fn
    let target = Chunk {
        kind: ChunkKind::Function,
        ident: "target".into(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&target).unwrap();

    // caller is a method `Foo::process`
    let caller = Chunk {
        kind: ChunkKind::Method,
        ident: "process".into(),
        parent: Some("Foo".into()),
        ..Chunk::stub(file_id)
    };
    let caller_id = db.insert_chunk(&caller).unwrap();
    let r = Reference {
        target_ident: "target".into(),
        line: 5,
        col: 1,
        ..Reference::stub(caller_id)
    };
    db.insert_ref(&r).unwrap();

    let result = build_callgraph(&db, "target").unwrap();
    assert_eq!(result.callers.len(), 1);
    assert_eq!(result.callers[0].ident, "process");
    assert_eq!(result.callers[0].parent.as_deref(), Some("Foo"));
}

#[test]
fn callgraph_emits_one_callee_per_distinct_parent_for_polysemic_idents() {
    let db = setup_test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();

    // The "owner" is a fn that calls `new`.
    let owner = Chunk {
        kind: ChunkKind::Function,
        ident: "owner".into(),
        ..Chunk::stub(file_id)
    };
    let owner_id = db.insert_chunk(&owner).unwrap();
    let r = Reference {
        target_ident: "new".into(),
        line: 5,
        col: 1,
        ..Reference::stub(owner_id)
    };
    db.insert_ref(&r).unwrap();

    // Two `new` definitions in the codebase: one on Foo, one on Bar.
    db.insert_chunk(&Chunk {
        kind: ChunkKind::Method,
        ident: "new".into(),
        parent: Some("Foo".into()),
        ..Chunk::stub(file_id)
    })
    .unwrap();
    db.insert_chunk(&Chunk {
        kind: ChunkKind::Method,
        ident: "new".into(),
        parent: Some("Bar".into()),
        ..Chunk::stub(file_id)
    })
    .unwrap();

    let result = build_callgraph(&db, "owner").unwrap();
    // Both Foo::new and Bar::new are listed — the static call graph
    // can't disambiguate, but exposing both candidates is more useful
    // than a bare "new" string.
    assert_eq!(result.callees.len(), 2);
    let mut parents: Vec<&str> = result
        .callees
        .iter()
        .filter_map(|c| c.parent.as_deref())
        .collect();
    parents.sort_unstable();
    assert_eq!(parents, vec!["Bar", "Foo"]);
    assert!(result.callees.iter().all(|c| c.ident == "new"));
}

#[test]
fn callgraph_external_callee_serialises_without_parent_key() {
    let db = setup_test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();

    let owner = Chunk {
        kind: ChunkKind::Function,
        ident: "owner".into(),
        ..Chunk::stub(file_id)
    };
    let owner_id = db.insert_chunk(&owner).unwrap();
    let r = Reference {
        target_ident: "println".into(), // not in our index
        line: 5,
        col: 1,
        ..Reference::stub(owner_id)
    };
    db.insert_ref(&r).unwrap();

    let result = build_callgraph(&db, "owner").unwrap();
    assert_eq!(result.callees.len(), 1);
    assert_eq!(result.callees[0].ident, "println");
    assert!(result.callees[0].parent.is_none());

    let json = serde_json::to_string(&result).unwrap();
    assert!(
        !json.contains("\"parent\""),
        "external callees with no candidate must not serialise the parent key, got {json}",
    );
}
