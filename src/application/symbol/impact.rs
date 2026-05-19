//! Impact analysis shared between CLI and MCP.

use std::path::Path;

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// A single location that would be impacted by changing a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactEntry {
    /// File path containing the reference.
    pub file: String,
    /// Symbol containing the reference.
    pub in_symbol: String,
    /// `Some(Type)` when the containing symbol is a method on a
    /// concrete type (`impl Type { fn in_symbol }`), `None` for free
    /// functions / module-level items. Disambiguates the
    /// **calling side** the same way `target_candidates`
    /// disambiguates the called side — without this, an
    /// `in_symbol = "new"` is just as ambiguous as a target named
    /// `new` would be.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_parent: Option<String>,
    /// Line number of the reference (1-indexed).
    pub line: u32,
    /// Byte column where the reference's ident starts (0-indexed,
    /// tree-sitter convention). Carries over from `Reference.col`
    /// so the parent-filter can disambiguate multiple same-line calls
    /// (`Foo::new(); Bar::new();`) by checking the literal at this
    /// exact position rather than line-wide substring matching.
    pub col: u32,
    /// Kind of reference (call, import, `type_use`).
    pub ref_kind: String,
}

/// One distinct chunk in the codebase that exposes the queried ident.
///
/// When the ident is unique (`render_widget`, `record_file_query`),
/// the result has exactly one candidate. When the ident is polysemous
/// (`new`, `as_str`, `open`), the result lists every concrete
/// `parent::ident` so the caller can disambiguate which target a ref
/// is actually pointing at — `rlm refs new` returning 485 hits is
/// only useful when the agent can see at a glance that those hits
/// resolve across `Vec`, `OperationResponse`, `RlmServer`, and
/// dozens of other parents.
#[derive(Debug, Clone, Serialize)]
pub struct TargetCandidate {
    /// `Some(Type)` for `impl Type { fn ident }`, `None` for free
    /// functions / module-level items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Chunk kind: `fn`, `method`, `struct`, `enum`, etc.
    pub kind: String,
    /// File the candidate lives in.
    pub file: String,
    /// 1-indexed start line of the candidate's chunk.
    pub line: u32,
}

/// Result of impact analysis for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactResult {
    /// The symbol being analyzed.
    pub symbol: String,
    /// Concrete targets matching the queried ident — one per chunk
    /// in the codebase that exposes this name. Empty when the ident
    /// has no defining chunk (e.g., querying a method that doesn't
    /// exist; the impacted list will then also be empty).
    pub target_candidates: Vec<TargetCandidate>,
    /// List of impacted locations.
    pub impacted: Vec<ImpactEntry>,
    /// Total count of impacted locations.
    pub count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

impl ImpactResult {
    /// Number of distinct files containing at least one impacted location.
    ///
    /// This is the count the savings middleware's `SymbolFiles` cost
    /// model needs for `files_touched` — using `count` would overstate
    /// `alt_calls` whenever multiple hits share a file.
    #[must_use]
    pub fn file_count(&self) -> u64 {
        use std::collections::HashSet;
        self.impacted
            .iter()
            .map(|e| e.file.as_str())
            .collect::<HashSet<_>>()
            .len() as u64
    }
}

/// Analyze the impact of changing a symbol.
///
/// Returns all locations (file, containing symbol, line, ref kind)
/// that reference this symbol and would need updating if it changes,
/// plus the set of concrete targets that share this ident (so polysemy
/// is visible at a glance, even before the caller filters on parent).
pub fn analyze_impact(db: &Database, symbol: &str) -> Result<ImpactResult> {
    // Single JOIN query instead of the legacy N+1 (get_chunk_by_id +
    // get_all_files per ref). See `Database::get_refs_with_context`.
    let refs_with_ctx = db.get_refs_with_context(symbol)?;

    let impacted: Vec<ImpactEntry> = refs_with_ctx
        .into_iter()
        .map(|rc| ImpactEntry {
            file: rc.file_path,
            in_symbol: rc.containing_symbol,
            in_parent: rc.containing_parent,
            line: rc.reference.line,
            col: rc.reference.col,
            ref_kind: rc.reference.ref_kind.as_str().to_string(),
        })
        .collect();

    let target_candidates = collect_target_candidates(db, symbol)?;

    let count = impacted.len();
    let mut result = ImpactResult {
        symbol: symbol.to_string(),
        target_candidates,
        impacted,
        count,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Restrict `result` to refs and candidates matching `parent`.
///
/// A `target_candidate` is kept iff its `parent == Some(parent)`. If
/// no candidate matches, both lists are emptied — there's no `Type`
/// in the codebase that even exposes this ident, so nothing can be
/// pointing at it.
///
/// An `impacted` ref is kept iff the source line at its location
/// contains the literal substring `<parent>::<symbol>` (the
/// path-qualified call form). Instance-method calls written as
/// `var.symbol(...)` cannot be type-traced without flow analysis;
/// they are intentionally dropped under this filter so the result
/// represents "definitely targets `parent::symbol`" rather than
/// "might target it". The caller can drop the filter if they need
/// the full list back.
pub fn filter_impacted_by_parent(
    result: &mut ImpactResult,
    parent: &str,
    project_root: &Path,
) -> Result<()> {
    result
        .target_candidates
        .retain(|c| c.parent.as_deref() == Some(parent));

    if result.target_candidates.is_empty() {
        result.impacted.clear();
        result.count = 0;
        result.tokens = estimate_output_tokens(&*result);
        return Ok(());
    }

    let entries = std::mem::take(&mut result.impacted);
    result.impacted = retain_path_call_entries(project_root, entries, parent, &result.symbol);
    result.count = result.impacted.len();
    result.tokens = estimate_output_tokens(&*result);
    Ok(())
}

/// Loop-invariant inputs threaded through [`mark_matching_lines`].
/// Bundles `source`, `parent`, and `symbol` so the matcher signature
/// stays under the SRP parameter ceiling — the per-(file, indices)
/// call site reads naturally as `mark_matching_lines(&matcher, ...)`.
struct PathCallMatcher<'a> {
    source: &'a str,
    parent: &'a str,
    symbol: &'a str,
}

/// Filter `entries` down to those whose source line carries
/// `<parent>::<symbol>` (in any of the Rust qualified-path shapes
/// — see [`line_carries_path_call`]) at the reference's exact
/// column. Groups by file so each source is read at most once.
fn retain_path_call_entries(
    project_root: &Path,
    entries: Vec<ImpactEntry>,
    parent: &str,
    symbol: &str,
) -> Vec<ImpactEntry> {
    let indices_by_file = group_entry_indices_by_file(&entries);
    let mut keep = vec![false; entries.len()];
    for (file, indices) in indices_by_file {
        let source = match std::fs::read_to_string(project_root.join(&file)) {
            Ok(s) => s,
            Err(e) => {
                // File was indexed but isn't readable now (deleted,
                // moved, permissions). Hard-failing would break
                // `rlm refs --parent` for every caller as soon as
                // one source file goes missing — far worse UX than
                // a partial result. Warn loudly on stderr (rlm
                // convention; matches staleness.rs) and skip just
                // this file's entries; the remaining `keep` bits
                // stay `false` so those refs drop out of the
                // filtered output rather than slipping through
                // un-vetted.
                eprintln!(
                    "rlm: refs --parent skipped {file}: {e} \
                     (indexed file no longer readable; re-run `rlm index .` to refresh)"
                );
                continue;
            }
        };
        let matcher = PathCallMatcher {
            source: &source,
            parent,
            symbol,
        };
        mark_matching_lines(&matcher, &entries, &indices, &mut keep);
    }
    entries
        .into_iter()
        .zip(keep)
        .filter_map(|(e, k)| k.then_some(e))
        .collect()
}

/// Group `entries`' positions by their `file` field so the file is
/// opened only once per group.
fn group_entry_indices_by_file(
    entries: &[ImpactEntry],
) -> std::collections::HashMap<String, Vec<usize>> {
    let mut by_file: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        by_file.entry(entry.file.clone()).or_default().push(i);
    }
    by_file
}

/// For each position in `indices`, flip `keep[i]` when the source
/// line at the reference's `(line, col)` carries `<parent>::<symbol>`
/// in any of the Rust qualified-path shapes the matcher recognises.
fn mark_matching_lines(
    matcher: &PathCallMatcher<'_>,
    entries: &[ImpactEntry],
    indices: &[usize],
    keep: &mut [bool],
) {
    let lines: Vec<&str> = matcher.source.lines().collect();
    for &i in indices {
        let entry = &entries[i];
        let line_no = entry.line.saturating_sub(1) as usize;
        let Some(text) = lines.get(line_no) else {
            continue;
        };
        if line_carries_path_call(text, entry.col as usize, matcher.parent, matcher.symbol) {
            keep[i] = true;
        }
    }
}

use super::path_match::line_carries_path_call;

/// Look up every chunk whose `ident` matches `symbol` and project
/// it into a [`TargetCandidate`]. Only the files actually referenced
/// by the matched chunks are loaded — avoids the
/// `get_all_files`-then-discard scan the previous shape did per call.
fn collect_target_candidates(db: &Database, symbol: &str) -> Result<Vec<TargetCandidate>> {
    let chunks = db.get_chunks_by_ident(symbol)?;
    let path_by_id = load_paths_for_chunks(db, &chunks)?;
    Ok(chunks
        .into_iter()
        .map(|c| build_candidate(c, &path_by_id))
        .collect())
}

/// Distinct file ids referenced by `chunks`, fetched in one batched
/// query.
fn load_paths_for_chunks(
    db: &Database,
    chunks: &[crate::domain::chunk::Chunk],
) -> Result<std::collections::HashMap<i64, String>> {
    let file_ids: std::collections::HashSet<i64> = chunks.iter().map(|c| c.file_id).collect();
    let ids: Vec<i64> = file_ids.into_iter().collect();
    let files = db.get_files_by_ids(&ids)?;
    Ok(files.into_iter().map(|f| (f.id, f.path)).collect())
}

/// Project one chunk into a [`TargetCandidate`] using the
/// pre-resolved file-path lookup. Missing file ids fall back to an
/// empty string — same behavior the old `get_all_files` path had.
fn build_candidate(
    c: crate::domain::chunk::Chunk,
    path_by_id: &std::collections::HashMap<i64, String>,
) -> TargetCandidate {
    TargetCandidate {
        parent: c.parent,
        kind: c.kind.as_str().to_string(),
        file: path_by_id.get(&c.file_id).cloned().unwrap_or_default(),
        line: c.start_line,
    }
}

#[cfg(test)]
#[path = "impact_ref_kind_tests.rs"]
mod ref_kind_tests;
#[cfg(test)]
#[path = "impact_tests.rs"]
mod tests;
