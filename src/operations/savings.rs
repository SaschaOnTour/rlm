//! Token savings tracking and reporting.
//!
//! Tracks how many tokens rlm saves compared to Claude Code's native tools
//! (Read, Grep, Glob). Each rlm operation is compared against what Claude Code
//! would have needed to achieve the same result.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;
use crate::models::token_estimate::{
    estimate_json_tokens, estimate_tokens, estimate_tokens_from_bytes,
};

/// Per-call overhead in tokens (tool_use block structure).
const CALL_OVERHEAD: u64 = 30;

/// Typical grep/read result snippet size in tokens.
const SNIPPET_TOKENS: u64 = 200;

/// API pricing ratio: input tokens cost per million.
const INPUT_COST_PER_M: u64 = 3;

/// API pricing ratio: output/call overhead cost per million.
const OVERHEAD_COST_PER_M: u64 = 15;

/// Line number overhead ratio (cat -n adds ~10% tokens).
const LINE_OVERHEAD_DIVISOR: u64 = 10;

/// Add Claude Code's line-number overhead (`N\t` prefix, ~10%) to a base token count.
fn with_line_overhead(base: u64) -> u64 {
    base.saturating_add(base / LINE_OVERHEAD_DIVISOR)
}

/// CC calls for Grep→Read→Edit (replace).
const CC_CALLS_REPLACE: u64 = 3;

/// CC calls for Read→Edit (insert).
const CC_CALLS_INSERT: u64 = 2;

// ─── Full round-trip savings entry ─────────────────────────────

/// Full round-trip savings record covering input tokens, output tokens,
/// and call counts for both the rlm path and the Claude Code alternative.
pub struct SavingsEntry {
    pub command: String,
    /// Tokens Claude sends as tool parameters (rlm side).
    pub rlm_input: u64,
    /// Tokens in rlm's response.
    pub rlm_output: u64,
    /// Number of rlm tool calls (always 1).
    pub rlm_calls: u64,
    /// Tokens Claude would send as tool parameters (CC side).
    pub alt_input: u64,
    /// Tokens in CC's tool results.
    pub alt_output: u64,
    /// Number of CC tool calls.
    pub alt_calls: u64,
    pub files_touched: u64,
}

impl SavingsEntry {
    /// Total tokens consumed on the rlm path.
    pub fn rlm_total(&self) -> u64 {
        self.rlm_input + self.rlm_output + self.rlm_calls * CALL_OVERHEAD
    }

    /// Total tokens consumed on the Claude Code path.
    pub fn alt_total(&self) -> u64 {
        self.alt_input + self.alt_output + self.alt_calls * CALL_OVERHEAD
    }

    /// Net tokens saved.
    pub fn saved(&self) -> u64 {
        self.alt_total().saturating_sub(self.rlm_total())
    }

    /// Savings as weighted cost in microdollars.
    ///
    /// Tool results become input tokens in subsequent turns, so both
    /// `alt_input`/`alt_output` use the input rate ($3/1M). Only call
    /// overhead uses the output rate ($15/1M) since Claude generates
    /// tool_use blocks as output tokens.
    // qual:api
    pub fn cost_saved_microdollars(&self) -> u64 {
        let alt_cost = self
            .alt_input
            .saturating_mul(INPUT_COST_PER_M)
            .saturating_add(self.alt_output.saturating_mul(INPUT_COST_PER_M))
            .saturating_add(
                self.alt_calls
                    .saturating_mul(CALL_OVERHEAD)
                    .saturating_mul(OVERHEAD_COST_PER_M),
            );
        let rlm_cost = self
            .rlm_input
            .saturating_mul(INPUT_COST_PER_M)
            .saturating_add(self.rlm_output.saturating_mul(INPUT_COST_PER_M))
            .saturating_add(
                self.rlm_calls
                    .saturating_mul(CALL_OVERHEAD)
                    .saturating_mul(OVERHEAD_COST_PER_M),
            );
        alt_cost.saturating_sub(rlm_cost)
    }
}

// ─── Report types ───────────────────────────────────────────────

/// Aggregate savings report.
#[derive(Debug, Clone, Serialize)]
pub struct SavingsReport {
    /// Total number of operations tracked.
    pub ops: u64,
    /// Total rlm output tokens (legacy, kept for compat).
    pub output: u64,
    /// Total CC output tokens (legacy, kept for compat).
    pub alternative: u64,
    /// Output-only savings (legacy, kept for compat).
    pub saved: u64,
    /// Output-only savings percentage (legacy).
    pub pct: f64,
    /// Full rlm cost (input + output + call overhead).
    pub rlm_total: u64,
    /// Full CC cost (input + output + call overhead).
    pub alt_total: u64,
    /// Full round-trip savings.
    pub total_saved: u64,
    /// Full round-trip savings percentage.
    pub total_pct: f64,
    /// Input token savings (alt_input - rlm_input).
    pub input_saved: u64,
    /// Result token savings (alt_output - rlm_output).
    pub result_saved: u64,
    /// Call count savings (alt_calls - rlm_calls).
    pub calls_saved: u64,
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
    /// Total rlm output tokens.
    pub output: u64,
    /// Total CC output tokens.
    pub alternative: u64,
    /// Output-only savings.
    pub saved: u64,
    /// Output-only savings percentage.
    pub pct: f64,
    /// CC call count total.
    pub alt_calls: u64,
    /// Full rlm cost.
    pub rlm_total: u64,
    /// Full CC cost.
    pub alt_total: u64,
}

// ─── Alternative cost estimation ────────────────────────────────

/// Estimate what Claude Code's Read(file) would cost for a single file.
///
/// Returns the estimated token count for reading the full file.
pub fn alternative_single_file(db: &Database, path: &str) -> Result<u64> {
    match db.get_file_by_path(path)? {
        Some(f) => Ok(with_line_overhead(estimate_tokens_from_bytes(f.size_bytes))),
        None => Ok(0),
    }
}

/// Estimate what Claude Code would need for a symbol-related operation.
///
/// Used for operations like `refs`, `callgraph`, `impact`, `context`, etc.
/// Claude Code would need to Grep for the symbol, then Read all involved files.
pub fn alternative_symbol_files(db: &Database, symbol: &str) -> Result<u64> {
    let total_bytes = db.get_symbol_file_sizes(symbol)?;
    Ok(with_line_overhead(estimate_tokens_from_bytes(total_bytes)))
}

// ─── Write operation cost helpers ───────────────────────────────

/// Full round-trip cost for Claude Code's Grep→Read→Edit to replace a symbol.
pub fn alternative_replace_entry(
    db: &Database,
    file_path: &str,
    old_code_len: usize,
    new_code_len: usize,
    rlm_result_len: usize,
) -> Result<SavingsEntry> {
    // DB has post-edit size after reindex; CC's Read sees the pre-edit file.
    let post_edit_bytes = db
        .get_file_by_path(file_path)?
        .map(|f| f.size_bytes)
        .unwrap_or(0);
    let pre_edit_bytes =
        (post_edit_bytes + old_code_len as u64).saturating_sub(new_code_len as u64);
    let file_tokens_with_lines = with_line_overhead(estimate_tokens_from_bytes(pre_edit_bytes));
    let old_tokens = estimate_tokens(old_code_len);
    let new_tokens = estimate_tokens(new_code_len);

    Ok(SavingsEntry {
        command: "replace".to_string(),
        // Parameter tokens only; per-call overhead is accounted for via rlm_calls.
        rlm_input: new_tokens,
        rlm_output: estimate_json_tokens(rlm_result_len),
        rlm_calls: 1,
        // CC: Grep(symbol) → Read(file) → Edit(old, new)
        // Parameter tokens only; per-call overhead is accounted for via alt_calls.
        alt_input: old_tokens + new_tokens, // Edit: old_string + new_string
        alt_output: SNIPPET_TOKENS            // Grep result (file matches)
            + file_tokens_with_lines          // Read result (full file + line numbers)
            + SNIPPET_TOKENS, // Edit result (patch + snippet)
        alt_calls: CC_CALLS_REPLACE,
        files_touched: 1,
    })
}

/// Full round-trip cost for Claude Code's Read→Edit to insert code.
pub fn alternative_insert_entry(
    db: &Database,
    file_path: &str,
    new_code_len: usize,
    rlm_result_len: usize,
) -> Result<SavingsEntry> {
    // DB has post-edit size after reindex; CC's Read sees the pre-edit file.
    let post_edit_bytes = db
        .get_file_by_path(file_path)?
        .map(|f| f.size_bytes)
        .unwrap_or(0);
    let pre_edit_bytes = post_edit_bytes.saturating_sub(new_code_len as u64);
    let file_tokens_with_lines = with_line_overhead(estimate_tokens_from_bytes(pre_edit_bytes));
    let new_tokens = estimate_tokens(new_code_len);

    Ok(SavingsEntry {
        command: "insert".to_string(),
        rlm_input: new_tokens,
        rlm_output: estimate_json_tokens(rlm_result_len),
        rlm_calls: 1,
        alt_input: new_tokens, // Edit: new_string (Read has negligible path param)
        alt_output: file_tokens_with_lines + SNIPPET_TOKENS, // Read result + Edit result
        alt_calls: CC_CALLS_INSERT,
        files_touched: 1,
    })
}

// ─── Recording ──────────────────────────────────────────────────

/// Record a full V2 savings entry (best-effort, errors are ignored).
pub fn record_v2(db: &Database, entry: &SavingsEntry) {
    let _ = db.record_savings_v2(
        &entry.command,
        entry.rlm_output,
        entry.alt_output,
        entry.files_touched,
        entry.rlm_input,
        entry.alt_input,
        entry.rlm_calls,
        entry.alt_calls,
    );
}

/// Record a savings entry (legacy wrapper — fills V2 columns with defaults).
pub fn record(
    db: &Database,
    command: &str,
    output_tokens: u64,
    alternative_tokens: u64,
    files_touched: u64,
) {
    let entry = SavingsEntry {
        command: command.to_string(),
        rlm_input: 0, // legacy: unknown parameter tokens
        rlm_output: output_tokens,
        rlm_calls: 1,
        alt_input: 0,
        alt_output: alternative_tokens,
        alt_calls: 1,
        files_touched,
    };
    record_v2(db, &entry);
}

/// Record savings for a read_symbol operation (CC equivalent: Grep + Read, 2 calls).
pub fn record_read_symbol(db: &Database, out_tokens: u64, path: &str) {
    let file_tokens = alternative_single_file(db, path).unwrap_or(0);
    let file_tokens = if file_tokens > 0 {
        file_tokens
    } else {
        out_tokens
    };
    let entry = SavingsEntry {
        command: "read_symbol".to_string(),
        rlm_input: 0, // negligible path/symbol params
        rlm_output: out_tokens,
        rlm_calls: 1,
        alt_input: 0,                             // negligible path/pattern params
        alt_output: SNIPPET_TOKENS + file_tokens, // Grep result + Read result
        alt_calls: 2,
        files_touched: 1,
    };
    record_v2(db, &entry);
}

/// Serialize a result, record savings with the given CC alternative profile, return JSON.
// qual:allow(srp_params) reason: "builds a SavingsEntry — params are the CC cost model, not decomposable"
fn serialize_and_record_entry<T: serde::Serialize>(
    db: &Database,
    command: &str,
    result: &T,
    alt_tokens: u64,
    alt_calls: u64,
    files_touched: u64,
) -> String {
    let json = serde_json::to_string(result)
        .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string());
    let out_tokens = estimate_json_tokens(json.len());
    let entry = SavingsEntry {
        command: command.to_string(),
        rlm_input: 0, // read-only ops have negligible params
        rlm_output: out_tokens,
        rlm_calls: 1,
        alt_input: 0,
        alt_output: alt_tokens,
        alt_calls,
        files_touched,
    };
    record_v2(db, &entry);
    json
}

/// Record savings for a file-scoped operation and return the serialized JSON.
///
/// CC equivalent: single Read call (alt_calls=1).
pub fn record_file_op<T: serde::Serialize>(
    db: &Database,
    command: &str,
    result: &T,
    path: &str,
) -> String {
    let json = serde_json::to_string(result)
        .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string());
    let out_tokens = estimate_json_tokens(json.len());
    // Fall back to out_tokens if file missing from DB or size unknown (no savings
    // assumed — we can't estimate CC's Read cost without knowing the actual file size).
    let alt_tokens = match alternative_single_file(db, path) {
        Ok(alt) if alt > 0 => alt,
        _ => out_tokens,
    };
    let entry = SavingsEntry {
        command: command.to_string(),
        rlm_input: 0,
        rlm_output: out_tokens,
        rlm_calls: 1,
        alt_input: 0,
        alt_output: alt_tokens,
        alt_calls: 1,
        files_touched: 1,
    };
    record_v2(db, &entry);
    json
}

/// Record savings for a symbol-scoped operation and return the serialized JSON.
///
/// CC equivalent: Grep (1 call) + Read per file (N calls).
///
/// **Note:** `files_touched` should be the number of *distinct files*, not total
/// hits. Some callers (e.g., `refs`) currently pass hit count — this overstates
/// `alt_calls` when multiple hits come from the same file.
pub fn record_symbol_op<T: serde::Serialize>(
    db: &Database,
    command: &str,
    result: &T,
    symbol: &str,
    files_touched: u64,
) -> String {
    let alt_tokens = alternative_symbol_files(db, symbol)
        .unwrap_or(0)
        .saturating_add(SNIPPET_TOKENS); // Grep result + Read results
    let alt_calls = 1 + files_touched; // Grep + Read×N
    serialize_and_record_entry(db, command, result, alt_tokens, alt_calls, files_touched)
}

/// Record savings for a scoped overview operation and return the serialized JSON.
///
/// CC would need Glob + Read×N files to get the same symbol info.
pub fn record_scoped_op<T: serde::Serialize>(
    db: &Database,
    command: &str,
    result: &T,
    path_prefix: Option<&str>,
) -> String {
    let (total_bytes, file_count) = db.get_scoped_file_stats(path_prefix).unwrap_or((0, 0));
    let alt_tokens =
        with_line_overhead(estimate_tokens_from_bytes(total_bytes)).saturating_add(SNIPPET_TOKENS); // Glob result + Read results
    let alt_calls = 1 + file_count; // Glob + Read×N
    serialize_and_record_entry(db, command, result, alt_tokens, alt_calls, file_count)
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
    // Compute input/call savings directly from raw query data (before consuming rows).
    let input_saved: u64 = by_cmd_raw
        .iter()
        .map(|r| r.alt_input_tokens.saturating_sub(r.rlm_input_tokens))
        .sum();
    let calls_saved: u64 = by_cmd_raw
        .iter()
        .map(|r| r.alt_calls.saturating_sub(r.rlm_calls))
        .sum();
    let by_cmd: Vec<CommandSavings> = by_cmd_raw
        .into_iter()
        .map(|row| {
            let cmd_saved = row.alt_tokens.saturating_sub(row.output_tokens);
            // Full round-trip totals
            let rlm_t = row.output_tokens + row.rlm_input_tokens + row.rlm_calls * CALL_OVERHEAD;
            let alt_t = row.alt_tokens + row.alt_input_tokens + row.alt_calls * CALL_OVERHEAD;
            CommandSavings {
                cmd: row.command,
                ops: row.ops,
                output: row.output_tokens,
                alternative: row.alt_tokens,
                saved: cmd_saved,
                pct: savings_pct(cmd_saved, row.alt_tokens),
                alt_calls: row.alt_calls,
                rlm_total: rlm_t,
                alt_total: alt_t,
            }
        })
        .collect();

    let ops: u64 = by_cmd.iter().map(|c| c.ops).sum();
    let output: u64 = by_cmd.iter().map(|c| c.output).sum();
    let alternative: u64 = by_cmd.iter().map(|c| c.alternative).sum();
    let saved = alternative.saturating_sub(output);
    let rlm_total: u64 = by_cmd.iter().map(|c| c.rlm_total).sum();
    let alt_total: u64 = by_cmd.iter().map(|c| c.alt_total).sum();
    let total_saved = alt_total.saturating_sub(rlm_total);
    let result_saved = saved; // output-only savings = result savings

    Ok(SavingsReport {
        ops,
        output,
        alternative,
        saved,
        pct: savings_pct(saved, alternative),
        rlm_total,
        alt_total,
        total_saved,
        total_pct: savings_pct(total_saved, alt_total),
        input_saved,
        result_saved,
        calls_saved,
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

    // ─── V2 tests ──────────────────────────────────────────────

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
        let line_overhead = PRE_EDIT_TOKENS / LINE_OVERHEAD_DIVISOR;
        let expected_alt_output = SNIPPET_TOKENS + PRE_EDIT_TOKENS + line_overhead + SNIPPET_TOKENS;
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
        let line_overhead = INSERT_PRE_EDIT_TOKENS / LINE_OVERHEAD_DIVISOR;
        let expected_alt_output = INSERT_PRE_EDIT_TOKENS + line_overhead + SNIPPET_TOKENS;
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
}
