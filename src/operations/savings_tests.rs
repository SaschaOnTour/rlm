//! Legacy-API tests for `savings.rs`.
//!
//! Split out of the inline `#[cfg(test)] mod tests { ... }` block and
//! then narrowed to the `record(..)` / `get_savings_report` surface.
//! V2-entry tests (`record_v2`, `alternative_*_entry`, scoped/symbol ops)
//! live in the sibling `savings_v2_tests.rs`. Wired back in via
//! `#[cfg(test)] #[path = "savings_tests.rs"] mod tests;`.

use super::fixtures::test_db;
use super::{alternative_single_file, alternative_symbol_files, get_savings_report, record};
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;

const PEEK_OUTPUT_TOKENS: u64 = 50;
const PEEK_ALT_TOKENS: u64 = 2000;
const PEEK_FILES_TOUCHED: u64 = 10;
const GREP_OUTPUT_TOKENS: u64 = 100;
const GREP_ALT_TOKENS: u64 = 100;
const GREP_FILES_TOUCHED: u64 = 5;
const PEEK2_OUTPUT_TOKENS: u64 = 60;
const PEEK2_ALT_TOKENS: u64 = 1500;
const PEEK2_FILES_TOUCHED: u64 = 8;
const SINGLE_FILE_SIZE: u64 = 4000;
const SCOPED_FILE_SIZE_A: u64 = 400;
const SCOPED_FILE_SIZE_B: u64 = 800;
const TEST_END_LINE: u32 = 5;
const TEST_END_LINE_SHORT: u32 = 3;
const TEST_END_BYTE: u32 = 50;
const TEST_END_BYTE_SMALL: u32 = 30;
const RECORD_ALT_TOKENS: u64 = 500;
const RECORD_FILES_TOUCHED: u64 = 3;
const TEST_REF_COL: u32 = 14;
const CMD_SAVINGS_OUTPUT_1: u64 = 50;
const CMD_SAVINGS_ALT_1: u64 = 1000;
const CMD_SAVINGS_OUTPUT_2: u64 = 30;
const CMD_SAVINGS_ALT_2: u64 = 800;

#[test]
fn savings_report_empty() {
    let db = test_db();
    let report = get_savings_report(&db, None).unwrap();
    assert_eq!(report.ops, 0);
    assert_eq!(report.output, 0);
    assert_eq!(report.alternative, 0);
    assert_eq!(report.saved, 0);
    assert!((report.pct - 0.0).abs() < f64::EPSILON);
    assert!(report.by_cmd.is_empty());
}

#[test]
fn savings_report_with_data() {
    let db = test_db();
    record(
        &db,
        "peek",
        PEEK_OUTPUT_TOKENS,
        PEEK_ALT_TOKENS,
        PEEK_FILES_TOUCHED,
    );
    record(
        &db,
        "grep",
        GREP_OUTPUT_TOKENS,
        GREP_ALT_TOKENS,
        GREP_FILES_TOUCHED,
    );
    record(
        &db,
        "peek",
        PEEK2_OUTPUT_TOKENS,
        PEEK2_ALT_TOKENS,
        PEEK2_FILES_TOUCHED,
    );

    let report = get_savings_report(&db, None).unwrap();
    assert_eq!(report.ops, 3);
    assert_eq!(report.output, 210);
    assert_eq!(report.alternative, 3600);
    assert_eq!(report.saved, 3390);
    assert!(report.pct > 90.0);
    assert_eq!(report.by_cmd.len(), 2);
}

#[test]
fn savings_report_with_since_filter() {
    let db = test_db();
    record(
        &db,
        "peek",
        PEEK_OUTPUT_TOKENS,
        PEEK_ALT_TOKENS,
        PEEK_FILES_TOUCHED,
    );

    // Future date should yield empty report
    let report = get_savings_report(&db, Some("2099-01-01")).unwrap();
    assert_eq!(report.ops, 0);

    // Past date should include everything
    let report = get_savings_report(&db, Some("2000-01-01")).unwrap();
    assert_eq!(report.ops, 1);
}

#[test]
fn savings_pct_zero_savings() {
    let db = test_db();
    record(
        &db,
        "grep",
        GREP_OUTPUT_TOKENS,
        GREP_ALT_TOKENS,
        GREP_FILES_TOUCHED,
    );

    let report = get_savings_report(&db, None).unwrap();
    assert_eq!(report.saved, 0);
    assert!((report.pct - 0.0).abs() < f64::EPSILON);
}

#[test]
fn alternative_single_file_known() {
    let db = test_db();
    let file = FileRecord::new(
        "src/main.rs".into(),
        "hash".into(),
        "rust".into(),
        SINGLE_FILE_SIZE,
    );
    db.upsert_file(&file).unwrap();

    let alt = alternative_single_file(&db, "src/main.rs").unwrap();
    assert_eq!(alt, 1100); // 4000 / 4 = 1000 + 10% line-number overhead
}

#[test]
fn alternative_single_file_unknown() {
    let db = test_db();
    let alt = alternative_single_file(&db, "nonexistent.rs").unwrap();
    assert_eq!(alt, 0);
}

#[test]
fn alternative_symbol_files_includes_defs_and_refs() {
    let db = test_db();
    let f1 = FileRecord::new(
        "src/a.rs".into(),
        "a".into(),
        "rust".into(),
        SCOPED_FILE_SIZE_A,
    );
    let f2 = FileRecord::new(
        "src/b.rs".into(),
        "b".into(),
        "rust".into(),
        SCOPED_FILE_SIZE_B,
    );
    let fid1 = db.upsert_file(&f1).unwrap();
    let fid2 = db.upsert_file(&f2).unwrap();

    // Symbol defined in f1
    let c1 = Chunk {
        end_line: TEST_END_LINE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "my_fn".into(),
        content: "fn my_fn() {}".into(),
        ..Chunk::stub(fid1)
    };
    db.insert_chunk(&c1).unwrap();

    // Caller in f2 references my_fn
    let c2 = Chunk {
        end_line: TEST_END_LINE_SHORT,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "caller".into(),
        content: "fn caller() { my_fn(); }".into(),
        ..Chunk::stub(fid2)
    };
    let cid2 = db.insert_chunk(&c2).unwrap();

    let r = Reference {
        id: 0,
        chunk_id: cid2,
        target_ident: "my_fn".into(),
        ref_kind: RefKind::Call,
        line: 1,
        col: TEST_REF_COL,
    };
    db.insert_ref(&r).unwrap();

    let alt = alternative_symbol_files(&db, "my_fn").unwrap();
    assert_eq!(alt, 330); // (400 + 800) / 4 = 300 + 10% line-number overhead
}

#[test]
fn record_best_effort_ignores_errors() {
    let db = test_db();
    // Should not panic even on normal usage
    record(
        &db,
        "test_cmd",
        GREP_OUTPUT_TOKENS,
        RECORD_ALT_TOKENS,
        RECORD_FILES_TOUCHED,
    );
    let (ops, _, _) = db.get_savings_totals(None).unwrap();
    assert_eq!(ops, 1);
}

#[test]
fn command_savings_percentage() {
    let db = test_db();
    record(
        &db,
        "read_symbol",
        CMD_SAVINGS_OUTPUT_1,
        CMD_SAVINGS_ALT_1,
        1,
    );
    record(
        &db,
        "read_symbol",
        CMD_SAVINGS_OUTPUT_2,
        CMD_SAVINGS_ALT_2,
        1,
    );

    let report = get_savings_report(&db, None).unwrap();
    let cmd = &report.by_cmd[0];
    assert_eq!(cmd.cmd, "read_symbol");
    assert_eq!(cmd.ops, 2);
    assert_eq!(cmd.output, 80);
    assert_eq!(cmd.alternative, 1800);
    assert_eq!(cmd.saved, 1720);
    // pct = 1720/1800 * 100 ≈ 95.56
    assert!(cmd.pct > 95.0);
}
