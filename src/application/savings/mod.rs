//! Token savings tracking and reporting.
//!
//! Tracks how many tokens rlm saves compared to Claude Code's native tools
//! (Read, Grep, Glob). Each rlm operation is compared against what Claude Code
//! would have needed to achieve the same result.

use crate::db::Database;
use crate::domain::savings::{
    with_line_overhead, SavingsEntry, CC_CALLS_EXTRACT, CC_CALLS_INSERT, CC_CALLS_REPLACE,
    SNIPPET_TOKENS,
};
use crate::domain::token_budget::{
    estimate_json_tokens, estimate_tokens, estimate_tokens_from_bytes,
};
use crate::error::Result;

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

/// Shared pre-edit size lookup: DB has the post-edit file size (reindex
/// already ran), CC's hypothetical Read saw the pre-edit file. Callers
/// pass `pre_minus_post_bytes` — the signed offset needed to recover
/// the pre-edit size:
/// * replace:        `old_code_len - new_code_len`
/// * delete:         `+ old_code_len`
/// * insert:         `- new_code_len`
/// * extract source: `+ bytes_moved`
/// * extract dest:   `- bytes_moved`
fn pre_edit_tokens_with_lines(
    db: &Database,
    file_path: &str,
    pre_minus_post_bytes: i64,
) -> Result<u64> {
    let post = db
        .get_file_by_path(file_path)?
        .map(|f| f.size_bytes)
        .unwrap_or(0);
    let pre = post.saturating_add_signed(pre_minus_post_bytes);
    Ok(with_line_overhead(estimate_tokens_from_bytes(pre)))
}

/// Full round-trip cost for Claude Code's Grep→Read→Edit to replace a symbol.
pub fn alternative_replace_entry(
    db: &Database,
    file_path: &str,
    old_code_len: usize,
    new_code_len: usize,
    rlm_result_len: usize,
) -> Result<SavingsEntry> {
    let file_tokens_with_lines =
        pre_edit_tokens_with_lines(db, file_path, old_code_len as i64 - new_code_len as i64)?;
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

/// Full round-trip cost for Claude Code's Grep→Read→Edit to delete a symbol.
/// Mirrors [`alternative_replace_entry`] but with `new_code = ""`.
pub fn alternative_delete_entry(
    db: &Database,
    file_path: &str,
    old_code_len: usize,
    rlm_result_len: usize,
) -> Result<SavingsEntry> {
    let file_tokens_with_lines = pre_edit_tokens_with_lines(db, file_path, old_code_len as i64)?;
    let old_tokens = estimate_tokens(old_code_len);

    Ok(SavingsEntry {
        command: "delete".to_string(),
        rlm_input: 0,
        rlm_output: estimate_json_tokens(rlm_result_len),
        rlm_calls: 1,
        alt_input: old_tokens,
        alt_output: SNIPPET_TOKENS + file_tokens_with_lines + SNIPPET_TOKENS,
        alt_calls: CC_CALLS_REPLACE,
        files_touched: 1,
    })
}

/// Full round-trip cost for Claude Code's Read→Edit→Read→Edit to
/// move symbols from one file to another. Extract is a two-file write
/// so the CC alternative reads both files, edits each, and touches
/// two files in one atomic call.
pub fn alternative_extract_entry(
    db: &Database,
    source_path: &str,
    dest_path: &str,
    bytes_moved: usize,
    rlm_result_len: usize,
) -> Result<SavingsEntry> {
    // Source had `bytes_moved` removed, destination received them —
    // sign flips between the two lookups.
    let source_tokens = pre_edit_tokens_with_lines(db, source_path, bytes_moved as i64)?;
    let dest_tokens = pre_edit_tokens_with_lines(db, dest_path, -(bytes_moved as i64))?;
    let moved_tokens = estimate_tokens(bytes_moved);

    Ok(SavingsEntry {
        command: "extract".to_string(),
        rlm_input: 0, // path + symbol list only
        rlm_output: estimate_json_tokens(rlm_result_len),
        rlm_calls: 1,
        // Edit(src): old=moved, new=""; Edit(dest): old="", new=moved.
        alt_input: moved_tokens.saturating_mul(2),
        alt_output: source_tokens                 // Read(src)
            + dest_tokens                         // Read(dest)
            + SNIPPET_TOKENS                      // Edit(src) result
            + SNIPPET_TOKENS, // Edit(dest) result
        alt_calls: CC_CALLS_EXTRACT,
        files_touched: 2,
    })
}

/// Full round-trip cost for Claude Code's Read→Edit to insert code.
pub fn alternative_insert_entry(
    db: &Database,
    file_path: &str,
    new_code_len: usize,
    rlm_result_len: usize,
) -> Result<SavingsEntry> {
    let file_tokens_with_lines = pre_edit_tokens_with_lines(db, file_path, -(new_code_len as i64))?;
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
    let _ = db.record_savings_v2(entry);
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

/// Claude Code cost-model profile for a single recorded operation.
/// Bundled so [`serialize_and_record_entry`] stays under the SRP
/// parameter ceiling — the three fields always travel together as
/// "what would CC have done equivalently".
struct SavingsProfile {
    alt_tokens: u64,
    alt_calls: u64,
    files_touched: u64,
}

/// Serialize a result, record savings with the given CC alternative profile, return JSON.
fn serialize_and_record_entry<T: serde::Serialize>(
    db: &Database,
    command: &str,
    result: &T,
    profile: &SavingsProfile,
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
        alt_output: profile.alt_tokens,
        alt_calls: profile.alt_calls,
        files_touched: profile.files_touched,
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
/// `files_touched` must be the number of *distinct files* involved; the
/// value is used directly as the Read count in the alt-path cost model
/// (`alt_calls = 1 + files_touched`). Passing a hit count instead would
/// overstate `alt_calls` whenever multiple hits come from the same file.
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
    serialize_and_record_entry(
        db,
        command,
        result,
        &SavingsProfile {
            alt_tokens,
            alt_calls,
            files_touched,
        },
    )
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
    serialize_and_record_entry(
        db,
        command,
        result,
        &SavingsProfile {
            alt_tokens,
            alt_calls,
            files_touched: file_count,
        },
    )
}

// ─── Reporting ──────────────────────────────────────────────────
//
// `get_savings_report` lives in `reporting.rs` to keep this module
// focused on recording.

pub mod reporting;
pub use reporting::get_savings_report;

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
#[path = "savings_fixtures_tests.rs"]
mod fixtures;
#[cfg(test)]
#[path = "savings_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "savings_v2_tests.rs"]
mod v2_tests;
