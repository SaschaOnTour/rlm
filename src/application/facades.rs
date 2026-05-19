//! Per-command application facades.
//!
//! Each public function wraps **one** adapter command: opens an
//! [`RlmSession`] for the given project root, calls the matching
//! session method, and returns the result. Adapters call exactly one
//! facade per handler — `[architecture.call_parity]` enforces this on
//! both the CLI and MCP surfaces so the two sides cannot silently
//! drift apart.
//!
//! Naming: `<command>_project`. The `_project` suffix disambiguates
//! the facade from the same-named session method and reads naturally
//! at call sites (`facades::search_project(&root, ...)`).

use std::path::Path;

use crate::application::content::partition;
use crate::application::edit::write_dispatch::{
    DeleteInput, ExtractInput, InsertInput, ReplaceInput, ReplaceMode, ReplaceOutput,
};
use crate::application::middleware::OperationResponse;
use crate::application::query::read::{ReadInputs, ReadOutput, ReadRequest};
use crate::application::query::{search::FieldsMode, stats, DetailLevel};
use crate::application::session::{RlmSession, VerifyOutput};
use crate::error::Result;

// ─── Read-side facades ───────────────────────────────────────────────

/// Full-text search across the indexed project.
pub fn search_project(
    project_root: &Path,
    query: &str,
    limit: usize,
    mode: FieldsMode,
) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.search(query, limit, mode)
}

/// Project-structure overview at one of three detail levels.
pub fn overview_project(
    project_root: &Path,
    detail: DetailLevel,
    path_filter: Option<&str>,
) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.overview(detail, path_filter)
}

/// Read a symbol body (optionally enriched with metadata) or a
/// Markdown section. Adapters pass their raw optional inputs; the
/// facade builds the typed [`ReadRequest`] (enforcing the
/// symbol-XOR-section invariant once) and dispatches — adapters
/// reach a single touchpoint.
pub fn read_project(project_root: &Path, inputs: &ReadInputs<'_>) -> Result<ReadOutput> {
    let request = ReadRequest::from_optional_inputs(inputs)?;
    let session = RlmSession::open(project_root)?;
    session.read(&request)
}

/// Symbol-impact analysis (where is `symbol` referenced?), optionally
/// filtered by the containing parent symbol.
pub fn refs_project(
    project_root: &Path,
    symbol: &str,
    parent: Option<&str>,
) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.refs(symbol, parent)
}

/// Symbol context: body + callers + callees, optionally with the
/// full callgraph slice.
pub fn context_project(
    project_root: &Path,
    symbol: &str,
    graph: bool,
) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.context(symbol, graph)
}

/// File-scoped dependencies (imports / use statements).
pub fn deps_project(project_root: &Path, path: &str) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.deps(path)
}

/// Symbols visible at a given line.
pub fn scope_project(project_root: &Path, path: &str, line: u32) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.scope(path, line)
}

/// Partition a file using a typed strategy.
pub fn partition_project(
    project_root: &Path,
    path: &str,
    strategy: partition::Strategy,
) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.partition(path, strategy)
}

/// Condensed file summary.
pub fn summarize_project(project_root: &Path, path: &str) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.summarize(path)
}

/// Diff a file (or single symbol) against the last-indexed version.
pub fn diff_project(
    project_root: &Path,
    path: &str,
    symbol: Option<&str>,
) -> Result<OperationResponse> {
    let session = RlmSession::open(project_root)?;
    session.diff(path, symbol)
}

// ─── Write-side facades ──────────────────────────────────────────────

/// Preview or apply a replace. The [`ReplaceMode`] selects the
/// branch; the typed [`ReplaceOutput`] tells the adapter which output
/// shape it received (`Preview(ReplaceDiff)` vs.
/// `Applied(String)`) so adapters dispatch on the variant rather than
/// branching on the input flag.
pub fn replace_project(
    project_root: &Path,
    input: &ReplaceInput<'_>,
    mode: ReplaceMode,
) -> Result<ReplaceOutput> {
    let session = RlmSession::open(project_root)?;
    session.replace(input, mode)
}

/// Delete a symbol (+ sidecar) and reindex the file.
pub fn delete_project(project_root: &Path, input: &DeleteInput<'_>) -> Result<String> {
    let session = RlmSession::open(project_root)?;
    session.delete(input)
}

/// Insert code at a given position and reindex.
pub fn insert_project(project_root: &Path, input: &InsertInput<'_>) -> Result<String> {
    let session = RlmSession::open(project_root)?;
    session.insert(input)
}

/// Move symbols to another file (atomic delete + insert).
pub fn extract_project(project_root: &Path, input: &ExtractInput<'_>) -> Result<String> {
    let session = RlmSession::open(project_root)?;
    session.extract(input)
}

// ─── Reporting facades ───────────────────────────────────────────────

/// Indexing stats or token-savings report.
pub fn stats_project(
    project_root: &Path,
    savings: bool,
    since: Option<&str>,
) -> Result<stats::StatsDispatchOutput> {
    let session = RlmSession::open(project_root)?;
    session.stats(savings, since)
}

/// Inspect parse-quality issues.
pub fn quality_project(
    project_root: &Path,
    flags: stats::QualityFlags,
) -> Result<stats::QualityBody> {
    let session = RlmSession::open(project_root)?;
    session.quality(flags)
}

/// Truncate the parse-quality log. Separate from `quality_project`
/// so the destructive surface is explicit on both adapters.
pub fn quality_clear_project(project_root: &Path) -> Result<stats::QualityClearedAck> {
    let session = RlmSession::open(project_root)?;
    session.quality_clear()
}

/// Verify index integrity, optionally auto-fixing recoverable issues.
pub fn verify_project(project_root: &Path, fix: bool) -> Result<VerifyOutput> {
    let session = RlmSession::open(project_root)?;
    session.verify(fix)
}

#[cfg(test)]
#[path = "facades_tests.rs"]
mod tests;
