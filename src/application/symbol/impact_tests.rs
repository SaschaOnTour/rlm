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
use crate::db::Database;
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
        target_candidates: Vec::new(),
        impacted: vec![
            ImpactEntry {
                in_parent: None,
                file: "src/a.rs".into(),
                in_symbol: "caller_a1".into(),
                line: 10,
                col: 0,
                ref_kind: "call".into(),
            },
            ImpactEntry {
                in_parent: None,
                file: "src/a.rs".into(),
                in_symbol: "caller_a2".into(),
                line: 20,
                col: 0,
                ref_kind: "call".into(),
            },
            ImpactEntry {
                in_parent: None,
                file: "src/b.rs".into(),
                in_symbol: "caller_b".into(),
                line: 5,
                col: 0,
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
        target_candidates: Vec::new(),
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

// ─── target_candidates + filter_impacted_by_parent ────────────────────

use super::filter_impacted_by_parent;

const PARENT_TEST_LINE: u32 = 30;

fn insert_method(db: &Database, file_id: i64, ident: &str, parent: &str) -> i64 {
    let chunk = Chunk {
        kind: ChunkKind::Method,
        ident: ident.into(),
        parent: Some(parent.into()),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&chunk).unwrap()
}

fn insert_free_fn(db: &Database, file_id: i64, ident: &str) -> i64 {
    let chunk = Chunk {
        kind: ChunkKind::Function,
        ident: ident.into(),
        parent: None,
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&chunk).unwrap()
}

/// Find the byte column where `path_call`'s ident starts on `line`.
/// `path_call` looks like `Foo::new` or `thing.new` — locate the
/// separator (`::` or `.`) and return the position right after it.
fn ident_col(line: &str, path_call: &str) -> u32 {
    let abs = line.find(path_call).expect("path_call must appear in line");
    let sep = path_call
        .rfind(':')
        .or_else(|| path_call.rfind('.'))
        .expect("path_call must contain '::' or '.'");
    (abs + sep + 1) as u32
}

fn insert_caller_with_ref(db: &Database, file_id: i64, target: &str, line: u32, col: u32) {
    let caller = Chunk {
        kind: ChunkKind::Function,
        ident: format!("caller_at_{line}_{col}"),
        ..Chunk::stub(file_id)
    };
    let caller_id = db.insert_chunk(&caller).unwrap();
    let r = Reference {
        target_ident: target.into(),
        line,
        col,
        ..Reference::stub(caller_id)
    };
    db.insert_ref(&r).unwrap();
}

#[test]
fn analyze_impact_lists_target_candidate_for_method() {
    let db = setup_test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method(&db, file_id, "new", "Foo");

    let result = analyze_impact(&db, "new").unwrap();

    assert_eq!(result.target_candidates.len(), 1);
    let c = &result.target_candidates[0];
    assert_eq!(c.parent.as_deref(), Some("Foo"));
    assert_eq!(c.kind, "method");
    assert_eq!(c.file, "src/x.rs");
}

#[test]
fn analyze_impact_target_candidate_omits_parent_for_free_fn() {
    let db = setup_test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_free_fn(&db, file_id, "helper");

    let result = analyze_impact(&db, "helper").unwrap();

    assert_eq!(result.target_candidates.len(), 1);
    assert!(result.target_candidates[0].parent.is_none());
}

#[test]
fn analyze_impact_lists_multiple_candidates_for_polysemic_ident() {
    let db = setup_test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method(&db, file_id, "new", "Foo");
    insert_method(&db, file_id, "new", "Bar");
    insert_method(&db, file_id, "new", "Baz");

    let result = analyze_impact(&db, "new").unwrap();

    assert_eq!(result.target_candidates.len(), 3);
    let mut parents: Vec<&str> = result
        .target_candidates
        .iter()
        .filter_map(|c| c.parent.as_deref())
        .collect();
    parents.sort_unstable();
    assert_eq!(parents, vec!["Bar", "Baz", "Foo"]);
}

#[test]
fn filter_keeps_only_path_qualified_calls_to_named_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("call.rs");
    let source = "fn ignored() {}\n\
fn first()  { let _ = Foo::new(); }\n\
fn second() { let bar = Bar::new(); }\n\
fn third()  { let _ = thing.new(); }\n";
    std::fs::write(&src, source).unwrap();

    let db = setup_test_db();
    let file = FileRecord::new("call.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method(&db, file_id, "new", "Foo");
    insert_method(&db, file_id, "new", "Bar");

    let lines: Vec<&str> = source.lines().collect();
    insert_caller_with_ref(&db, file_id, "new", 2, ident_col(lines[1], "Foo::new"));
    insert_caller_with_ref(&db, file_id, "new", 3, ident_col(lines[2], "Bar::new"));
    insert_caller_with_ref(&db, file_id, "new", 4, ident_col(lines[3], "thing.new"));

    let mut result = analyze_impact(&db, "new").unwrap();
    assert_eq!(result.impacted.len(), 3);

    filter_impacted_by_parent(&mut result, "Foo", tmp.path()).unwrap();

    assert_eq!(result.target_candidates.len(), 1);
    assert_eq!(result.target_candidates[0].parent.as_deref(), Some("Foo"));
    assert_eq!(result.impacted.len(), 1);
    assert_eq!(result.impacted[0].line, 2);
    assert_eq!(result.count, 1);
}

#[test]
fn filter_with_unknown_parent_clears_both_lists() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("call.rs"), "// nothing\n").unwrap();

    let db = setup_test_db();
    let file = FileRecord::new("call.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method(&db, file_id, "new", "Foo");
    // No filter call follows, so col doesn't influence assertions.
    insert_caller_with_ref(&db, file_id, "new", PARENT_TEST_LINE, 0);

    let mut result = analyze_impact(&db, "new").unwrap();
    filter_impacted_by_parent(&mut result, "DoesNotExist", tmp.path()).unwrap();

    assert!(result.target_candidates.is_empty());
    assert!(result.impacted.is_empty());
    assert_eq!(result.count, 0);
}

#[test]
fn impact_entry_carries_in_parent_for_method_callers() {
    let db = setup_test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method(&db, file_id, "target", "TargetType");
    // Caller is a method on Foo, not a free function — `in_parent` must surface that.
    let caller = Chunk {
        kind: ChunkKind::Method,
        ident: "caller".into(),
        parent: Some("Foo".into()),
        ..Chunk::stub(file_id)
    };
    let caller_id = db.insert_chunk(&caller).unwrap();
    let r = Reference {
        target_ident: "target".into(),
        line: 7,
        col: 1,
        ..Reference::stub(caller_id)
    };
    db.insert_ref(&r).unwrap();

    let result = analyze_impact(&db, "target").unwrap();

    assert_eq!(result.impacted.len(), 1);
    assert_eq!(
        result.impacted[0].in_parent.as_deref(),
        Some("Foo"),
        "impact entries must surface the caller's parent so the calling-side polysemy is visible too — symmetric to target_candidates",
    );
}

#[test]
fn impact_entry_omits_in_parent_for_free_function_callers() {
    let db = setup_test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method(&db, file_id, "target", "TargetType");
    // No filter call follows, so col doesn't influence assertions.
    insert_caller_with_ref(&db, file_id, "target", 5, 0); // caller is a free fn

    let result = analyze_impact(&db, "target").unwrap();

    assert_eq!(result.impacted.len(), 1);
    assert!(
        result.impacted[0].in_parent.is_none(),
        "free-fn callers have no parent — the field must be None",
    );
}

#[test]
fn filter_disambiguates_multiple_same_line_calls_by_col() {
    // Two calls on the same line (`Foo::new(); Bar::new();`) emit two
    // refs at different columns. Without col-aware filtering, the old
    // line-substring check would keep both refs for `--parent Foo`
    // because the line as a whole contains `Foo::new`. The new
    // positional check rejects Bar::new at its own column.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("multi.rs");
    let source = "fn one() { let _ = Foo::new(); let _ = Bar::new(); }\n\
fn two() { let _ = Bar::new(); }\n";
    std::fs::write(&src, source).unwrap();

    let db = setup_test_db();
    let file = FileRecord::new("multi.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method(&db, file_id, "new", "Foo");
    insert_method(&db, file_id, "new", "Bar");

    let lines: Vec<&str> = source.lines().collect();
    let foo_col_l1 = ident_col(lines[0], "Foo::new");
    let bar_col_l1 = ident_col(lines[0], "Bar::new");
    let bar_col_l2 = ident_col(lines[1], "Bar::new");

    insert_caller_with_ref(&db, file_id, "new", 1, foo_col_l1);
    insert_caller_with_ref(&db, file_id, "new", 1, bar_col_l1);
    insert_caller_with_ref(&db, file_id, "new", 2, bar_col_l2);

    let mut result = analyze_impact(&db, "new").unwrap();
    assert_eq!(result.impacted.len(), 3);

    filter_impacted_by_parent(&mut result, "Foo", tmp.path()).unwrap();

    assert_eq!(
        result.impacted.len(),
        1,
        "only Foo::new on line 1 should survive --parent Foo; \
         got {:?}",
        result.impacted,
    );
    let kept = &result.impacted[0];
    assert_eq!(kept.line, 1);
    assert_eq!(kept.col, foo_col_l1);
}

// `line_carries_path_call` lives in the sibling `path_match` module
// alongside its own companion tests (`path_match_tests.rs`).
// `filter_disambiguates_multiple_same_line_calls_by_col` above
// exercises it end-to-end through `filter_impacted_by_parent`.
