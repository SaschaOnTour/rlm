//! Single-file / fixed-cost middleware tests for `savings_recorder.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "savings_recorder_tests.rs"] mod tests;`.
//!
//! Scoped / symbol-files tests live in the sibling
//! `savings_recorder_scoped_tests.rs`.

use super::super::fixtures::{payload, test_db, Payload};
use super::{record_operation, AlternativeCost, OperationMeta};
use crate::domain::file::FileRecord;

const FILE_SIZE: u64 = 4_000;

#[test]
fn records_single_file_op_and_returns_json_body() {
    let db = test_db();
    let file = FileRecord::new("src/main.rs".into(), "abc".into(), "rust".into(), FILE_SIZE);
    db.upsert_file(&file).unwrap();

    let meta = OperationMeta {
        command: "summarize",
        files_touched: 1,
        alternative: AlternativeCost::SingleFile {
            path: "src/main.rs".into(),
        },
    };

    let response = record_operation(&db, &meta, &payload("hello"));
    assert!(response.body.contains("hello"));
    assert!(!response.body.contains('\n'), "body must be minified JSON");
    assert!(response.tokens_out > 0);

    let rows = db.get_savings_by_command(None).unwrap();
    let entry = rows
        .iter()
        .find(|r| r.command == "summarize")
        .expect("summarize entry recorded");
    assert_eq!(entry.ops, 1);
}

#[test]
fn fixed_alternative_uses_precomputed_token_count() {
    const FIXED_TOKENS: u64 = 500;

    let db = test_db();
    let meta = OperationMeta {
        command: "search",
        files_touched: 2,
        alternative: AlternativeCost::Fixed(FIXED_TOKENS),
    };

    let response = record_operation(&db, &meta, &payload("s"));
    assert!(!response.body.is_empty());

    let rows = db.get_savings_by_command(None).unwrap();
    let row = rows
        .iter()
        .find(|r| r.command == "search")
        .expect("search entry");
    assert_eq!(row.alt_tokens, FIXED_TOKENS);
    // Regression guard: Fixed and AtLeastBody must record through the
    // V2-aware savings::record, not the bare Database::record_savings
    // INSERT. Otherwise rlm_calls/alt_calls stay NULL and the savings
    // report undercounts call overhead.
    assert_eq!(row.rlm_calls, 1);
    assert_eq!(row.alt_calls, 1);
}

#[test]
fn at_least_body_clamps_up_to_body_size() {
    // base smaller than actual body tokens → alt = body tokens.
    const SMALL_BASE: u64 = 1;

    let db = test_db();
    let meta = OperationMeta {
        command: "search",
        files_touched: 1,
        alternative: AlternativeCost::AtLeastBody { base: SMALL_BASE },
    };

    // Use a payload large enough to exceed SMALL_BASE in estimated tokens.
    let big = Payload {
        label: "x".repeat(200),
    };
    let response = record_operation(&db, &meta, &big);

    let rows = db.get_savings_by_command(None).unwrap();
    let row = rows
        .iter()
        .find(|r| r.command == "search")
        .expect("search entry");
    // alt must be at least the body's token count, which exceeds SMALL_BASE.
    assert!(row.alt_tokens >= response.tokens_out);
    assert!(row.alt_tokens > SMALL_BASE);
}

#[test]
fn at_least_body_keeps_base_when_larger_than_body() {
    // base larger than body tokens → alt = base.
    const LARGE_BASE: u64 = 10_000;

    let db = test_db();
    let meta = OperationMeta {
        command: "search",
        files_touched: 1,
        alternative: AlternativeCost::AtLeastBody { base: LARGE_BASE },
    };

    let response = record_operation(&db, &meta, &payload("s"));
    assert!(response.tokens_out < LARGE_BASE);

    let rows = db.get_savings_by_command(None).unwrap();
    let row = rows
        .iter()
        .find(|r| r.command == "search")
        .expect("search entry");
    assert_eq!(row.alt_tokens, LARGE_BASE);
    // Same regression guard as the Fixed test.
    assert_eq!(row.rlm_calls, 1);
    assert_eq!(row.alt_calls, 1);
}

#[test]
fn body_is_always_minified_json() {
    let db = test_db();
    let meta = OperationMeta {
        command: "fixed",
        files_touched: 0,
        alternative: AlternativeCost::Fixed(10),
    };

    let response = record_operation(&db, &meta, &payload("min"));
    // Middleware returns raw minified JSON regardless of the adapter
    // downstream: no newlines, no pretty-printing.
    assert!(!response.body.contains('\n'));
}
