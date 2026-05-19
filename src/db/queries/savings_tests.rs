//! Tests for `savings.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "savings_tests.rs"] mod tests;`.

use crate::db::Database;
use crate::domain::file::FileRecord;
use crate::domain::savings::SavingsEntry;

const TEST_FILE_SIZE_A: u64 = 400;
const TEST_FILE_SIZE_B: u64 = 800;

#[test]
fn record_savings_v2_writes_entry_fields_via_struct() {
    // Verifies the new struct-based signature: `record_savings_v2(&SavingsEntry)`
    // (replaces the prior 8-positional-param form).
    let db = Database::open_in_memory().unwrap();
    let entry = SavingsEntry {
        command: "probe_v2".to_string(),
        rlm_input: 10,
        rlm_output: 100,
        rlm_calls: 1,
        alt_input: 20,
        alt_output: 200,
        alt_calls: 2,
        files_touched: 3,
    };
    db.record_savings_v2(&entry).unwrap();

    // Round-trip via the aggregate query.
    let rows = db.get_savings_by_command(None).unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.command, "probe_v2");
    assert_eq!(row.output_tokens, 100);
    assert_eq!(row.alt_tokens, 200);
    assert_eq!(row.rlm_input_tokens, 10);
    assert_eq!(row.alt_input_tokens, 20);
    assert_eq!(row.rlm_calls, 1);
    assert_eq!(row.alt_calls, 2);
}

#[test]
fn scoped_file_stats() {
    let db = Database::open_in_memory().unwrap();
    let f1 = FileRecord::new(
        "src/a.rs".into(),
        "a".into(),
        "rust".into(),
        TEST_FILE_SIZE_A,
    );
    let f2 = FileRecord::new(
        "tests/t.rs".into(),
        "b".into(),
        "rust".into(),
        TEST_FILE_SIZE_B,
    );
    db.upsert_file(&f1).unwrap();
    db.upsert_file(&f2).unwrap();

    let (size, count) = db.get_scoped_file_stats(None).unwrap();
    assert_eq!(count, 2);
    assert_eq!(size, TEST_FILE_SIZE_A + TEST_FILE_SIZE_B);

    let (size, count) = db.get_scoped_file_stats(Some("src/")).unwrap();
    assert_eq!(count, 1);
    assert_eq!(size, TEST_FILE_SIZE_A);

    let (_, count) = db.get_scoped_file_stats(Some("nonexistent/")).unwrap();
    assert_eq!(count, 0);
}
