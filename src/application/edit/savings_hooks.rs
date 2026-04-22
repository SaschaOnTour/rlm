//! Shared savings-recording hooks for write operations.
//!
//! Both the CLI and the MCP adapter need to log the token-savings entry
//! that belongs to every successful `replace` / `delete` / `insert`.
//! The bookkeeping (build `SavingsEntry` → swallow errors → `record_v2`)
//! used to live inline in each adapter, which forced both handlers to
//! import the savings recorder directly — a duplication the 0.5.0
//! consolidation drove out.
//!
//! This module is the application-layer seam: adapters call exactly one
//! of these helpers after a successful write; the underlying
//! [`crate::application::savings`] API stays inside the application
//! layer where it belongs.
//!
//! All helpers are best-effort: a failure to compute or persist the
//! savings entry must never mask a successful write, so errors are
//! swallowed here (same contract as `savings::record_v2`).

use crate::application::savings;
use crate::db::Database;

/// Record savings after a successful `replace`.
pub fn record_replace(
    db: &Database,
    path: &str,
    old_code_len: usize,
    new_code_len: usize,
    result_json_len: usize,
) {
    if let Ok(entry) =
        savings::alternative_replace_entry(db, path, old_code_len, new_code_len, result_json_len)
    {
        savings::record_v2(db, &entry);
    }
}

/// Record savings after a successful `delete`.
pub fn record_delete(db: &Database, path: &str, old_code_len: usize, result_json_len: usize) {
    if let Ok(entry) = savings::alternative_delete_entry(db, path, old_code_len, result_json_len) {
        savings::record_v2(db, &entry);
    }
}

/// Record savings after a successful `insert`.
pub fn record_insert(db: &Database, path: &str, new_code_len: usize, result_json_len: usize) {
    if let Ok(entry) = savings::alternative_insert_entry(db, path, new_code_len, result_json_len) {
        savings::record_v2(db, &entry);
    }
}

/// Record savings after a successful `extract`. Unlike the single-file
/// operations above, extract touches a source and a destination file —
/// the entry's `files_touched = 2` reflects that.
pub fn record_extract(
    db: &Database,
    source_path: &str,
    dest_path: &str,
    bytes_moved: usize,
    result_json_len: usize,
) {
    if let Ok(entry) =
        savings::alternative_extract_entry(db, source_path, dest_path, bytes_moved, result_json_len)
    {
        savings::record_v2(db, &entry);
    }
}
