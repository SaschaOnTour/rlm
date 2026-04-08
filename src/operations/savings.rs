//! Token savings tracking and reporting.
//!
//! Tracks how many tokens rlm saves compared to Claude Code's native tools
//! (Read, Grep, Glob). Each rlm operation is compared against what Claude Code
//! would have needed to achieve the same result.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;
use crate::models::token_estimate::estimate_tokens_from_bytes;

// ─── Report types ───────────────────────────────────────────────

/// Aggregate savings report.
#[derive(Debug, Clone, Serialize)]
pub struct SavingsReport {
    /// Total number of operations tracked.
    pub ops: u64,
    /// Total rlm output tokens.
    pub output: u64,
    /// Total tokens Claude Code tools would have consumed.
    pub alternative: u64,
    /// Tokens saved (alternative - output).
    pub saved: u64,
    /// Savings percentage.
    pub pct: f64,
    /// Breakdown by command.
    pub by_cmd: Vec<CommandSavings>,
}

/// Per-command savings breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct CommandSavings {
    /// Command/feature name.
    pub cmd: String,
    /// Number of invocations.
    pub ops: u64,
    /// Total output tokens.
    pub output: u64,
    /// Total alternative tokens.
    pub alternative: u64,
    /// Tokens saved.
    pub saved: u64,
    /// Savings percentage.
    pub pct: f64,
}

// ─── Alternative cost estimation ────────────────────────────────

/// Estimate what Claude Code's Read(file) would cost for a single file.
///
/// Returns the estimated token count for reading the full file.
pub fn alternative_single_file(db: &Database, path: &str) -> Result<u64> {
    match db.get_file_by_path(path)? {
        Some(f) => Ok(estimate_tokens_from_bytes(f.size_bytes)),
        None => Ok(0),
    }
}

/// Estimate what Claude Code would need for reading all files in scope.
///
/// Used for operations like `peek`, `map`, `tree` where Claude Code would
/// need to Glob + Read every file to get symbol information.
pub fn alternative_scoped_files(db: &Database, path_prefix: Option<&str>) -> Result<u64> {
    let total_bytes = db.get_scoped_file_sizes(path_prefix)?;
    Ok(estimate_tokens_from_bytes(total_bytes))
}

/// Estimate what Claude Code would need for a symbol-related operation.
///
/// Used for operations like `refs`, `callgraph`, `impact`, `context`, etc.
/// Claude Code would need to Grep for the symbol, then Read all involved files.
pub fn alternative_symbol_files(db: &Database, symbol: &str) -> Result<u64> {
    let total_bytes = db.get_symbol_file_sizes(symbol)?;
    Ok(estimate_tokens_from_bytes(total_bytes))
}

// ─── Recording ──────────────────────────────────────────────────

/// Record a savings entry (best-effort, errors are ignored).
pub fn record(
    db: &Database,
    command: &str,
    output_tokens: u64,
    alternative_tokens: u64,
    files_touched: u64,
) {
    let _ = db.record_savings(command, output_tokens, alternative_tokens, files_touched);
}

/// Record savings for a file-scoped operation and return the serialized JSON.
///
/// Compares the JSON output size against what Claude Code's Read would need
/// for the same file. Returns the JSON string for printing/returning.
pub fn record_file_op<T: serde::Serialize>(
    db: &Database,
    command: &str,
    result: &T,
    path: &str,
) -> String {
    let json = serde_json::to_string(result).unwrap_or_default();
    let out_tokens = crate::models::token_estimate::estimate_tokens(json.len());
    let alt_tokens = alternative_single_file(db, path).unwrap_or(out_tokens);
    record(db, command, out_tokens, alt_tokens, 1);
    json
}

/// Record savings for a symbol-scoped operation and return the serialized JSON.
///
/// Compares the JSON output size against what Claude Code would need
/// to Grep + Read all files containing the symbol.
pub fn record_symbol_op<T: serde::Serialize>(
    db: &Database,
    command: &str,
    result: &T,
    symbol: &str,
    files_touched: u64,
) -> String {
    let json = serde_json::to_string(result).unwrap_or_default();
    let out_tokens = crate::models::token_estimate::estimate_tokens(json.len());
    let alt_tokens = alternative_symbol_files(db, symbol).unwrap_or(0);
    record(db, command, out_tokens, alt_tokens, files_touched);
    json
}

/// Record savings for a scoped overview operation and return the serialized JSON.
///
/// Compares against what Claude Code would need to Glob + Read all files in scope.
pub fn record_scoped_op<T: serde::Serialize>(
    db: &Database,
    command: &str,
    result: &T,
    path_prefix: Option<&str>,
) -> String {
    let json = serde_json::to_string(result).unwrap_or_default();
    let out_tokens = crate::models::token_estimate::estimate_tokens(json.len());
    let alt_tokens = alternative_scoped_files(db, path_prefix).unwrap_or(0);
    record(db, command, out_tokens, alt_tokens, 0);
    json
}

/// Multiplier to convert a ratio to a percentage.
const PERCENT: f64 = 100.0;

// ─── Reporting ──────────────────────────────────────────────────

/// Calculate savings percentage.
fn savings_pct(saved: u64, alternative: u64) -> f64 {
    if alternative > 0 {
        (saved as f64 / alternative as f64) * PERCENT
    } else {
        0.0
    }
}

/// Generate a savings report, optionally filtered by date.
///
/// Derives aggregate totals from the per-command breakdown (single DB query).
pub fn get_savings_report(db: &Database, since: Option<&str>) -> Result<SavingsReport> {
    let by_cmd_raw = db.get_savings_by_command(since)?;
    let by_cmd: Vec<CommandSavings> = by_cmd_raw
        .into_iter()
        .map(|(cmd, cmd_ops, cmd_output, cmd_alt)| {
            let cmd_saved = cmd_alt.saturating_sub(cmd_output);
            CommandSavings {
                cmd,
                ops: cmd_ops,
                output: cmd_output,
                alternative: cmd_alt,
                saved: cmd_saved,
                pct: savings_pct(cmd_saved, cmd_alt),
            }
        })
        .collect();

    let ops: u64 = by_cmd.iter().map(|c| c.ops).sum();
    let output: u64 = by_cmd.iter().map(|c| c.output).sum();
    let alternative: u64 = by_cmd.iter().map(|c| c.alternative).sum();
    let saved = alternative.saturating_sub(output);

    Ok(SavingsReport {
        ops,
        output,
        alternative,
        saved,
        pct: savings_pct(saved, alternative),
        by_cmd,
    })
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;

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

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

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
        record(&db, "peek", PEEK_OUTPUT_TOKENS, PEEK_ALT_TOKENS, PEEK_FILES_TOUCHED);
        record(&db, "grep", GREP_OUTPUT_TOKENS, GREP_ALT_TOKENS, GREP_FILES_TOUCHED);
        record(&db, "peek", PEEK2_OUTPUT_TOKENS, PEEK2_ALT_TOKENS, PEEK2_FILES_TOUCHED);

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
        record(&db, "peek", PEEK_OUTPUT_TOKENS, PEEK_ALT_TOKENS, PEEK_FILES_TOUCHED);

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
        record(&db, "grep", GREP_OUTPUT_TOKENS, GREP_ALT_TOKENS, GREP_FILES_TOUCHED);

        let report = get_savings_report(&db, None).unwrap();
        assert_eq!(report.saved, 0);
        assert!((report.pct - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn alternative_single_file_known() {
        let db = test_db();
        let file = FileRecord::new("src/main.rs".into(), "hash".into(), "rust".into(), SINGLE_FILE_SIZE);
        db.upsert_file(&file).unwrap();

        let alt = alternative_single_file(&db, "src/main.rs").unwrap();
        assert_eq!(alt, 1000); // 4000 / 4
    }

    #[test]
    fn alternative_single_file_unknown() {
        let db = test_db();
        let alt = alternative_single_file(&db, "nonexistent.rs").unwrap();
        assert_eq!(alt, 0);
    }

    #[test]
    fn alternative_scoped_files_all() {
        let db = test_db();
        let f1 = FileRecord::new("src/a.rs".into(), "a".into(), "rust".into(), SCOPED_FILE_SIZE_A);
        let f2 = FileRecord::new("src/b.rs".into(), "b".into(), "rust".into(), SCOPED_FILE_SIZE_B);
        db.upsert_file(&f1).unwrap();
        db.upsert_file(&f2).unwrap();

        let alt = alternative_scoped_files(&db, None).unwrap();
        assert_eq!(alt, 300); // (SCOPED_FILE_SIZE_A + SCOPED_FILE_SIZE_B) / 4
    }

    #[test]
    fn alternative_scoped_files_filtered() {
        let db = test_db();
        let f1 = FileRecord::new("src/a.rs".into(), "a".into(), "rust".into(), SCOPED_FILE_SIZE_A);
        let f2 = FileRecord::new("tests/t.rs".into(), "b".into(), "rust".into(), SCOPED_FILE_SIZE_B);
        db.upsert_file(&f1).unwrap();
        db.upsert_file(&f2).unwrap();

        let alt = alternative_scoped_files(&db, Some("src/")).unwrap();
        assert_eq!(alt, 100); // SCOPED_FILE_SIZE_A / 4
    }

    #[test]
    fn alternative_symbol_files_includes_defs_and_refs() {
        let db = test_db();
        let f1 = FileRecord::new("src/a.rs".into(), "a".into(), "rust".into(), SCOPED_FILE_SIZE_A);
        let f2 = FileRecord::new("src/b.rs".into(), "b".into(), "rust".into(), SCOPED_FILE_SIZE_B);
        let fid1 = db.upsert_file(&f1).unwrap();
        let fid2 = db.upsert_file(&f2).unwrap();

        // Symbol defined in f1
        let c1 = Chunk {
            id: 0,
            file_id: fid1,
            start_line: 1,
            end_line: TEST_END_LINE,
            start_byte: 0,
            end_byte: TEST_END_BYTE,
            kind: ChunkKind::Function,
            ident: "my_fn".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn my_fn() {}".into(),
        };
        db.insert_chunk(&c1).unwrap();

        // Caller in f2 references my_fn
        let c2 = Chunk {
            id: 0,
            file_id: fid2,
            start_line: 1,
            end_line: TEST_END_LINE_SHORT,
            start_byte: 0,
            end_byte: TEST_END_BYTE_SMALL,
            kind: ChunkKind::Function,
            ident: "caller".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn caller() { my_fn(); }".into(),
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
        assert_eq!(alt, 300); // (SCOPED_FILE_SIZE_A + SCOPED_FILE_SIZE_B) / 4
    }

    #[test]
    fn record_best_effort_ignores_errors() {
        let db = test_db();
        // Should not panic even on normal usage
        record(&db, "test_cmd", GREP_OUTPUT_TOKENS, RECORD_ALT_TOKENS, RECORD_FILES_TOUCHED);
        let (ops, _, _) = db.get_savings_totals(None).unwrap();
        assert_eq!(ops, 1);
    }

    #[test]
    fn command_savings_percentage() {
        let db = test_db();
        record(&db, "read_symbol", CMD_SAVINGS_OUTPUT_1, CMD_SAVINGS_ALT_1, 1);
        record(&db, "read_symbol", CMD_SAVINGS_OUTPUT_2, CMD_SAVINGS_ALT_2, 1);

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
}
