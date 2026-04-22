//! Shared write-operation dispatchers for CLI and MCP.
//!
//! Every write-side tool (`replace` / `delete` / `insert` / `extract`)
//! shares the same orchestration: call the underlying `replacer` /
//! `inserter` / `extractor` primitive, reindex the touched file(s),
//! splice op-specific fields into the JSON envelope, and record
//! savings. Before 0.5.0 that orchestration was duplicated inside
//! each adapter. The dispatchers in this module are the single
//! application-layer entry point both adapters call, so new fields
//! (sidecar lines, extract destinations, …) land once instead of
//! twice.
//!
//! Keep this module boring: parse args → call primitive → build
//! envelope → record savings → return. Adapters only own
//! argument-parsing and output-channel selection.

use std::path::Path;

use super::extractor::{extract_symbols, ExtractOutcome};
use super::inserter::{insert_code, InsertPosition};
use super::replacer::{delete_symbol, preview_replace, replace_symbol, ReplaceDiff};
use super::savings_hooks;
use super::validator::SyntaxGuard;
use crate::application::index::{self, PreviewSource};
use crate::config::Config;
use crate::db::Database;
use crate::error::Result;

// ─── Replace ─────────────────────────────────────────────────────────

/// Arguments shared by `replace` preview + apply paths. Grouped so the
/// dispatcher signatures fit the SRP parameter budget.
pub struct ReplaceInput<'a> {
    pub path: &'a str,
    pub symbol: &'a str,
    pub parent: Option<&'a str>,
    pub code: &'a str,
}

/// Preview a replace: returns the typed diff for the adapter to
/// serialise through its own formatter.
pub fn dispatch_replace_preview(db: &Database, input: &ReplaceInput<'_>) -> Result<ReplaceDiff> {
    preview_replace(db, input.path, input.symbol, input.parent, input.code)
}

/// Apply a replace: call the replacer, reindex, record savings, return
/// the pre-serialised JSON envelope.
pub fn dispatch_replace_apply(
    db: &Database,
    config: &Config,
    input: &ReplaceInput<'_>,
) -> Result<String> {
    let outcome = replace_symbol(
        db,
        input.path,
        input.symbol,
        input.parent,
        input.code,
        &config.project_root,
    )?;
    let result_json =
        index::reindex_with_result(db, config, input.path, PreviewSource::Symbol(input.symbol));
    savings_hooks::record_replace(
        db,
        input.path,
        outcome.old_code_len,
        input.code.len(),
        result_json.len(),
    );
    Ok(result_json)
}

// ─── Delete ──────────────────────────────────────────────────────────

/// Arguments for `dispatch_delete`.
pub struct DeleteInput<'a> {
    pub path: &'a str,
    pub symbol: &'a str,
    pub parent: Option<&'a str>,
    pub keep_docs: bool,
}

/// Delete a symbol, reindex, splice sidecar-line info if the adjacent
/// doc/attr block was removed, record savings.
pub fn dispatch_delete(db: &Database, config: &Config, input: &DeleteInput<'_>) -> Result<String> {
    let outcome = delete_symbol(
        db,
        input.path,
        input.symbol,
        input.parent,
        input.keep_docs,
        &config.project_root,
    )?;

    let base_json =
        index::reindex_with_result(db, config, input.path, PreviewSource::Symbol(input.symbol));
    let result_json = splice_delete_sidecar(&base_json, outcome.sidecar_lines)?;

    savings_hooks::record_delete(db, input.path, outcome.old_code_len, result_json.len());
    Ok(result_json)
}

/// Add a `deleted.sidecar_lines` field when the delete also removed a
/// leading doc-comment / attribute block. The base envelope is
/// produced by our own `reindex_with_result`, so a parse failure
/// here would mean that function emitted malformed JSON — an
/// internal bug we want to surface, not swallow.
fn splice_delete_sidecar(base_json: &str, sidecar: Option<(u32, u32)>) -> Result<String> {
    let Some((from, to)) = sidecar else {
        return Ok(base_json.to_string());
    };
    let mut value: serde_json::Value = serde_json::from_str(base_json)?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "deleted".to_string(),
            serde_json::json!({ "sidecar_lines": [from, to] }),
        );
    }
    Ok(serde_json::to_string(&value)?)
}

// ─── Insert ──────────────────────────────────────────────────────────

/// Arguments for `dispatch_insert`. `db` is optional because a fresh
/// project without an index can still receive inserts — the response
/// just advertises `reindexed: false` with a helpful hint.
pub struct InsertInput<'a> {
    pub path: &'a str,
    pub position: &'a InsertPosition,
    pub code: &'a str,
}

/// Insert code, then — if an index exists — reindex + record savings.
/// Returns the pre-serialised JSON envelope both adapters emit.
pub fn dispatch_insert(
    db: Option<&Database>,
    project_root: &Path,
    input: &InsertInput<'_>,
) -> Result<String> {
    let guard = SyntaxGuard::new();
    insert_code(project_root, input.path, input.position, input.code, &guard)?;

    let Some(db) = db else {
        return Ok(serde_json::json!({
            "ok": true,
            "reindexed": false,
            "hint": "no index; call 'index' to enable auto-reindex",
        })
        .to_string());
    };

    let config = Config::new(project_root);
    let result_json =
        index::reindex_with_result(db, &config, input.path, input.position.preview_source());
    savings_hooks::record_insert(db, input.path, input.code.len(), result_json.len());
    Ok(result_json)
}

// ─── Extract ─────────────────────────────────────────────────────────

/// Arguments for `dispatch_extract`.
pub struct ExtractInput<'a> {
    pub path: &'a str,
    pub symbols: &'a [String],
    pub to: &'a str,
    pub parent: Option<&'a str>,
}

/// Extract symbols from `path` into `to`, reindexing both files and
/// splicing `source` / `dest` / `extracted` / `dest_reindex` fields
/// into the response envelope.
pub fn dispatch_extract(
    db: &Database,
    config: &Config,
    input: &ExtractInput<'_>,
) -> Result<String> {
    let outcome = extract_symbols(
        db,
        input.path,
        input.symbols,
        input.to,
        input.parent,
        &config.project_root,
    )?;

    let source_json = index::reindex_with_result(db, config, input.path, PreviewSource::None);
    let dest_json = index::reindex_with_result(db, config, input.to, PreviewSource::None);

    splice_extract_envelope(&source_json, &dest_json, input.path, input.to, &outcome)
}

/// Both envelope JSONs were produced by our own `reindex_with_result`;
/// a parse failure on either is an internal bug and surfaces as an
/// error rather than a silent "`reindexed: false`" lie.
fn splice_extract_envelope(
    source_json: &str,
    dest_json: &str,
    source_path: &str,
    dest_path: &str,
    outcome: &ExtractOutcome,
) -> Result<String> {
    let mut response: serde_json::Value = serde_json::from_str(source_json)?;
    let dest_val: serde_json::Value = serde_json::from_str(dest_json)?;
    if let Some(obj) = response.as_object_mut() {
        obj.insert(
            "source".to_string(),
            serde_json::Value::String(source_path.into()),
        );
        obj.insert(
            "dest".to_string(),
            serde_json::Value::String(dest_path.into()),
        );
        obj.insert("extracted".to_string(), serde_json::to_value(outcome)?);
        obj.insert("dest_reindex".to_string(), dest_val);
    }
    Ok(response.to_string())
}
