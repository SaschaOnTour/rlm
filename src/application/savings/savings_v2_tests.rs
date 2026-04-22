//! V2-entry savings tests for `savings.rs`.
//!
//! Split out of `savings_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). The legacy `record(..)`
//! API tests remain in `savings_tests.rs`; this file covers the V2
//! `SavingsEntry` / `record_v2` / `alternative_*_entry` / `record_scoped_op`
//! / `record_symbol_op` surfaces.

use super::fixtures::test_db;
use super::{
    alternative_extract_entry, alternative_insert_entry, alternative_replace_entry,
    estimate_tokens_from_bytes, get_savings_report, record, record_scoped_op, record_symbol_op,
    record_v2, with_line_overhead, CC_CALLS_EXTRACT, CC_CALLS_INSERT, CC_CALLS_REPLACE,
    SNIPPET_TOKENS,
};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;
use crate::domain::savings::SavingsEntry;

const TEST_END_LINE: u32 = 5;
const TEST_END_BYTE: u32 = 50;
const SCOPED_FILE_SIZE_A: u64 = 400;
const SCOPED_FILE_SIZE_B: u64 = 800;

// Pre-edit adjustment test constants
const REPLACE_POST_EDIT_SIZE: u64 = 4300;
const REPLACE_OLD_CODE: usize = 100;
const REPLACE_NEW_CODE: usize = 400;
const PRE_EDIT_TOKENS: u64 = 1000; // (4300+100-400)/4 = 4000/4
const INSERT_POST_EDIT_SIZE: u64 = 4200;
const INSERT_NEW_CODE: usize = 200;
const INSERT_PRE_EDIT_TOKENS: u64 = 1000; // (4200-200)/4 = 4000/4
const SYMBOL_FILES_TOUCHED: u64 = 5;

// V2 test constants
const V2_FILE_SIZE: u64 = 4000;
const V2_OLD_CODE_LEN: usize = 100;
const V2_NEW_CODE_LEN: usize = 200;
const V2_RESULT_LEN: usize = 10;
const V2_RLM_INPUT: u64 = 50; // ceil(200/4) = new_tokens only (no call overhead)
const V2_RLM_OUTPUT: u64 = 5; // ceil(10/2) = JSON result at 2 bytes/token
const V2_ALT_INPUT_REPLACE: u64 = 75; // ceil(100/4) + ceil(200/4) = old_tokens + new_tokens
const V2_ALT_OUTPUT_REPLACE: u64 = 1472; // pre-edit: (4000+100-200)/4=975, overhead=97, 200+975+97+200
const V2_ALT_INPUT_INSERT: u64 = 50; // ceil(200/4) = new_tokens only
const V2_ALT_OUTPUT_INSERT: u64 = 1245; // pre-edit: (4000-200)/4=950, overhead=95, 950+95+200
const LEGACY_OUTPUT: u64 = 50;
const LEGACY_ALT: u64 = 2000;
const LEGACY_FILES: u64 = 10;

#[test]
fn savings_entry_totals() {
    let entry = SavingsEntry {
        command: "test".into(),
        rlm_input: 80,
        rlm_output: 10,
        rlm_calls: 1,
        alt_input: 165,
        alt_output: 1500,
        alt_calls: 3,
        files_touched: 1,
    };
    // rlm: 80 + 10 + 1*30 = 120
    assert_eq!(entry.rlm_total(), 120);
    // alt: 165 + 1500 + 3*30 = 1755
    assert_eq!(entry.alt_total(), 1755);
    // saved: 1755 - 120 = 1635
    assert_eq!(entry.saved(), 1635);
}

#[test]
fn savings_entry_cost_microdollars() {
    let entry = SavingsEntry {
        command: "test".into(),
        rlm_input: 100,
        rlm_output: 10,
        rlm_calls: 1,
        alt_input: 200,
        alt_output: 1000,
        alt_calls: 3,
        files_touched: 1,
    };
    // alt_cost = 200*3 + 1000*3 + 3*30*15 = 600 + 3000 + 1350 = 4950
    // rlm_cost = 100*3 + 10*3 + 1*30*15 = 300 + 30 + 450 = 780
    // saved = 4950 - 780 = 4170
    assert_eq!(entry.cost_saved_microdollars(), 4170);
}

#[test]
fn replace_entry_full_roundtrip() {
    let db = test_db();
    let file = FileRecord::new(
        "src/main.rs".into(),
        "h".into(),
        "rust".into(),
        V2_FILE_SIZE,
    );
    db.upsert_file(&file).unwrap();

    let entry = alternative_replace_entry(
        &db,
        "src/main.rs",
        V2_OLD_CODE_LEN,
        V2_NEW_CODE_LEN,
        V2_RESULT_LEN,
    )
    .unwrap();

    assert_eq!(entry.rlm_input, V2_RLM_INPUT);
    assert_eq!(entry.rlm_output, V2_RLM_OUTPUT);
    assert_eq!(entry.rlm_calls, 1);
    assert_eq!(entry.alt_input, V2_ALT_INPUT_REPLACE);
    assert_eq!(entry.alt_output, V2_ALT_OUTPUT_REPLACE);
    assert_eq!(entry.alt_calls, CC_CALLS_REPLACE);
}

/// Extract is a two-file write: the entry reads both files, edits
/// each, and the `files_touched` count must be 2. `alt_calls` is the
/// `CC_CALLS_EXTRACT = 4` (Read+Edit on src, Read+Edit on dest).
#[test]
fn extract_entry_roundtrip_charges_both_files() {
    let db = test_db();
    let source = FileRecord::new(
        "src/source.rs".into(),
        "h".into(),
        "rust".into(),
        V2_FILE_SIZE,
    );
    let dest = FileRecord::new(
        "src/dest.rs".into(),
        "h".into(),
        "rust".into(),
        V2_NEW_CODE_LEN as u64,
    );
    db.upsert_file(&source).unwrap();
    db.upsert_file(&dest).unwrap();

    // Source got `bytes_moved` removed, dest received it — mirrors
    // the post-extract state.
    let bytes_moved = V2_NEW_CODE_LEN;
    let entry = alternative_extract_entry(
        &db,
        "src/source.rs",
        "src/dest.rs",
        bytes_moved,
        V2_RESULT_LEN,
    )
    .unwrap();

    assert_eq!(entry.files_touched, 2, "extract spans source + dest");
    assert_eq!(entry.alt_calls, CC_CALLS_EXTRACT);
    assert_eq!(entry.rlm_calls, 1);
    assert!(
        entry.alt_output > entry.rlm_output,
        "CC alternative must cost more — it reads two files + two edits"
    );
}

#[test]
fn insert_entry_full_roundtrip() {
    let db = test_db();
    let file = FileRecord::new(
        "src/main.rs".into(),
        "h".into(),
        "rust".into(),
        V2_FILE_SIZE,
    );
    db.upsert_file(&file).unwrap();

    let entry =
        alternative_insert_entry(&db, "src/main.rs", V2_NEW_CODE_LEN, V2_RESULT_LEN).unwrap();

    assert_eq!(entry.rlm_input, V2_RLM_INPUT);
    assert_eq!(entry.rlm_output, V2_RLM_OUTPUT);
    assert_eq!(entry.rlm_calls, 1);
    assert_eq!(entry.alt_input, V2_ALT_INPUT_INSERT);
    assert_eq!(entry.alt_output, V2_ALT_OUTPUT_INSERT);
    assert_eq!(entry.alt_calls, CC_CALLS_INSERT);
}

#[test]
fn legacy_record_fills_defaults() {
    let db = test_db();
    record(&db, "peek", LEGACY_OUTPUT, LEGACY_ALT, LEGACY_FILES);
    // Should work and produce valid report
    let report = get_savings_report(&db, None).unwrap();
    assert_eq!(report.ops, 1);
    // Legacy record sets rlm_input=0, alt_input=0, calls=1
    // rlm_total = 0 + 50 + 30 = 80, alt_total = 0 + 2000 + 30 = 2030
    assert_eq!(report.rlm_total, 80);
    assert_eq!(report.alt_total, 2030);
    assert_eq!(report.total_saved, 1950);
}

#[test]
fn report_includes_v2_fields() {
    let db = test_db();
    let entry = SavingsEntry {
        command: "replace".into(),
        rlm_input: V2_RLM_INPUT,
        rlm_output: V2_RLM_OUTPUT,
        rlm_calls: 1,
        alt_input: V2_ALT_INPUT_REPLACE,
        alt_output: V2_ALT_OUTPUT_REPLACE,
        alt_calls: CC_CALLS_REPLACE,
        files_touched: 1,
    };
    record_v2(&db, &entry);

    let report = get_savings_report(&db, None).unwrap();
    assert_eq!(report.ops, 1);
    // rlm_total = 50 + 5 + 30 = 85
    assert_eq!(report.rlm_total, 85);
    // alt_total = 75 + 1472 + 90 = 1637
    assert_eq!(report.alt_total, 1637);
    assert_eq!(report.total_saved, 1552);
    assert!(report.total_pct > 90.0);
    assert_eq!(report.input_saved, 25); // 75 - 50
    assert_eq!(report.result_saved, 1467); // 1472 - 5
    assert_eq!(report.calls_saved, 2); // 3 - 1

    let cmd = &report.by_cmd[0];
    assert_eq!(cmd.alt_calls, CC_CALLS_REPLACE);
    assert_eq!(cmd.rlm_total, 85);
    assert_eq!(cmd.alt_total, 1637);
}

#[test]
fn replace_entry_uses_pre_edit_file_size() {
    let db = test_db();
    // File in DB has post-edit size (after reindex). The replacement grew the file.
    let file = FileRecord::new(
        "src/grow.rs".into(),
        "h".into(),
        "rust".into(),
        REPLACE_POST_EDIT_SIZE,
    );
    db.upsert_file(&file).unwrap();

    let entry = alternative_replace_entry(
        &db,
        "src/grow.rs",
        REPLACE_OLD_CODE,
        REPLACE_NEW_CODE,
        V2_RESULT_LEN,
    )
    .unwrap();

    // Pre-edit size = 4300 + 100 - 400 = 4000 → tokens = 1000
    let expected_alt_output = SNIPPET_TOKENS + with_line_overhead(PRE_EDIT_TOKENS) + SNIPPET_TOKENS;
    assert_eq!(entry.alt_output, expected_alt_output);
}

#[test]
fn insert_entry_uses_pre_edit_file_size() {
    let db = test_db();
    // Post-edit size after inserting INSERT_NEW_CODE bytes.
    let file = FileRecord::new(
        "src/grow.rs".into(),
        "h".into(),
        "rust".into(),
        INSERT_POST_EDIT_SIZE,
    );
    db.upsert_file(&file).unwrap();

    let entry =
        alternative_insert_entry(&db, "src/grow.rs", INSERT_NEW_CODE, V2_RESULT_LEN).unwrap();

    // Pre-edit size = 4200 - 200 = 4000 → tokens = 1000
    let expected_alt_output = with_line_overhead(INSERT_PRE_EDIT_TOKENS) + SNIPPET_TOKENS;
    assert_eq!(entry.alt_output, expected_alt_output);
}

#[test]
fn symbol_op_alt_calls_scales_with_files() {
    let db = test_db();
    // Set up two files with the same symbol so alternative_symbol_files returns something.
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
    let c1 = Chunk {
        kind: ChunkKind::Function,
        ident: "shared_fn".into(),
        end_line: TEST_END_LINE,
        end_byte: TEST_END_BYTE,
        content: "fn shared_fn() {}".into(),
        ..Chunk::stub(fid1)
    };
    let c2 = Chunk {
        kind: ChunkKind::Function,
        ident: "shared_fn".into(),
        end_line: TEST_END_LINE,
        end_byte: TEST_END_BYTE,
        content: "fn shared_fn() {}".into(),
        ..Chunk::stub(fid2)
    };
    db.insert_chunk(&c1).unwrap();
    db.insert_chunk(&c2).unwrap();

    // Record with files_touched=5 (Grep + 5 Reads = 6 CC calls)
    let data = serde_json::json!({"test": true});
    record_symbol_op(&db, "refs", &data, "shared_fn", SYMBOL_FILES_TOUCHED);

    let report = get_savings_report(&db, None).unwrap();
    let cmd = &report.by_cmd[0];
    assert_eq!(cmd.alt_calls, 1 + SYMBOL_FILES_TOUCHED); // Grep + Read×5

    // With 0 files, only 1 Grep call
    let db2 = test_db();
    record_symbol_op(&db2, "refs", &data, "nonexistent", 0);
    let report2 = get_savings_report(&db2, None).unwrap();
    assert_eq!(report2.by_cmd[0].alt_calls, 1); // Grep only
}

#[test]
fn cost_saved_microdollars_saturates_on_overflow() {
    let entry = SavingsEntry {
        command: "huge".into(),
        rlm_input: 0,
        rlm_output: 0,
        rlm_calls: 0,
        alt_input: u64::MAX,
        alt_output: u64::MAX,
        alt_calls: u64::MAX,
        files_touched: 0,
    };
    // Should not panic or wrap — saturating arithmetic clamps to u64::MAX.
    let cost = entry.cost_saved_microdollars();
    assert_eq!(cost, u64::MAX);
}

#[test]
fn backwards_compat_zero_columns() {
    let db = test_db();
    // Simulate old-style insert (rlm_input=0, alt_input=0 from defaults)
    db.record_savings("peek", LEGACY_OUTPUT, LEGACY_ALT, LEGACY_FILES)
        .unwrap();

    let report = get_savings_report(&db, None).unwrap();
    assert_eq!(report.ops, 1);
    assert_eq!(report.output, LEGACY_OUTPUT);
    assert_eq!(report.alternative, LEGACY_ALT);
    assert_eq!(report.saved, LEGACY_ALT - LEGACY_OUTPUT);
    // Old rows have rlm_input=0, alt_input=0, rlm_calls=1, alt_calls=1
    // rlm_total = 50 + 0 + 30 = 80, alt_total = 2000 + 0 + 30 = 2030
    assert_eq!(report.rlm_total, 80);
    assert_eq!(report.alt_total, 2030);
    assert_eq!(report.total_saved, 1950);
    // No division by zero
    assert!(report.total_pct > 0.0);
}

#[test]
fn scoped_op_counts_files_for_alt_calls() {
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
    db.upsert_file(&f1).unwrap();
    db.upsert_file(&f2).unwrap();

    let data = serde_json::json!({"test": true});
    record_scoped_op(&db, "overview", &data, Some("src/"));

    let report = get_savings_report(&db, None).unwrap();
    let cmd = &report.by_cmd[0];
    assert_eq!(cmd.alt_calls, 3); // Glob + Read×2

    // alt_tokens = with_line_overhead(total_bytes/4) + SNIPPET_TOKENS
    // Both files are in src/: 400+800=1200 bytes → 300 tokens → +10% = 330 → +200 snippet = 530
    let expected_alt = with_line_overhead(estimate_tokens_from_bytes(
        SCOPED_FILE_SIZE_A + SCOPED_FILE_SIZE_B,
    )) + SNIPPET_TOKENS;
    assert_eq!(cmd.alternative, expected_alt);
}
