//! Basic tests for `impact.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "impact_tests.rs"] mod tests;`.
//!
//! RefKind-centric tests (type-use, cross-file impact) live in the
//! sibling `impact_ref_kind_tests.rs` to keep each companion focused
//! on a smaller cluster of behaviors (SRP_MODULE).

use super::super::fixtures::setup_test_db;
use super::{analyze_impact, ImpactEntry, ImpactResult, TokenEstimate};
use crate::domain::chunk::{Chunk, ChunkKind, Reference};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 200;
const TARGET_START_LINE: u32 = 50;
const TARGET_END_LINE: u32 = 60;
const TARGET_START_BYTE: u32 = 500;
const TARGET_END_BYTE: u32 = 600;
const CALLER1_START_LINE: u32 = 10;
const CALLER1_END_LINE: u32 = 20;
const CALLER1_START_BYTE: u32 = 100;
const CALLER1_END_BYTE: u32 = 200;
const CALLER2_START_LINE: u32 = 30;
const CALLER2_END_LINE: u32 = 40;
const CALLER2_START_BYTE: u32 = 300;
const CALLER2_END_BYTE: u32 = 400;
const CALLER1_REF_LINE: u32 = 15;
const CALLER2_REF_LINE: u32 = 35;
const TEST_REF_COL: u32 = 5;

#[test]
fn file_count_deduplicates_hits_per_file() {
    let result = ImpactResult {
        symbol: "foo".into(),
        impacted: vec![
            ImpactEntry {
                file: "src/a.rs".into(),
                in_symbol: "caller_a1".into(),
                line: 10,
                ref_kind: "call".into(),
            },
            ImpactEntry {
                file: "src/a.rs".into(),
                in_symbol: "caller_a2".into(),
                line: 20,
                ref_kind: "call".into(),
            },
            ImpactEntry {
                file: "src/b.rs".into(),
                in_symbol: "caller_b".into(),
                line: 5,
                ref_kind: "call".into(),
            },
        ],
        count: 3,
        tokens: TokenEstimate::default(),
    };
    // 3 hits across 2 distinct files.
    assert_eq!(result.count, 3);
    assert_eq!(result.file_count(), 2);
}

#[test]
fn file_count_is_zero_for_empty_result() {
    let result = ImpactResult {
        symbol: "foo".into(),
        impacted: Vec::new(),
        count: 0,
        tokens: TokenEstimate::default(),
    };
    assert_eq!(result.file_count(), 0);
}

#[test]
fn test_impact_empty_symbol() {
    let db = setup_test_db();
    let result = analyze_impact(&db, "nonexistent").unwrap();

    assert_eq!(result.symbol, "nonexistent");
    assert!(result.impacted.is_empty());
    assert_eq!(result.count, 0);
}

#[test]
fn test_impact_basic() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/utils.rs".to_string(),
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
        ident: "helper".to_string(),
        signature: Some("fn helper()".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn helper() { }".to_string(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&target).unwrap();

    let caller1 = Chunk {
        start_line: CALLER1_START_LINE,
        end_line: CALLER1_END_LINE,
        start_byte: CALLER1_START_BYTE,
        end_byte: CALLER1_END_BYTE,
        kind: ChunkKind::Function,
        ident: "process".to_string(),
        signature: Some("fn process()".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn process() { helper(); }".to_string(),
        ..Chunk::stub(file_id)
    };
    let caller1_id = db.insert_chunk(&caller1).unwrap();

    let caller2 = Chunk {
        start_line: CALLER2_START_LINE,
        end_line: CALLER2_END_LINE,
        start_byte: CALLER2_START_BYTE,
        end_byte: CALLER2_END_BYTE,
        kind: ChunkKind::Function,
        ident: "handle".to_string(),
        signature: Some("fn handle()".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn handle() { helper(); }".to_string(),
        ..Chunk::stub(file_id)
    };
    let caller2_id = db.insert_chunk(&caller2).unwrap();

    let ref1 = Reference {
        target_ident: "helper".to_string(),
        line: CALLER1_REF_LINE,
        col: TEST_REF_COL,
        ..Reference::stub(caller1_id)
    };
    db.insert_ref(&ref1).unwrap();

    let ref2 = Reference {
        target_ident: "helper".to_string(),
        line: CALLER2_REF_LINE,
        col: TEST_REF_COL,
        ..Reference::stub(caller2_id)
    };
    db.insert_ref(&ref2).unwrap();

    let result = analyze_impact(&db, "helper").unwrap();

    assert_eq!(result.symbol, "helper");
    assert_eq!(result.count, 2);
    assert_eq!(result.impacted.len(), 2);

    let symbols: Vec<&str> = result
        .impacted
        .iter()
        .map(|e| e.in_symbol.as_str())
        .collect();
    assert!(symbols.contains(&"process"));
    assert!(symbols.contains(&"handle"));
}
