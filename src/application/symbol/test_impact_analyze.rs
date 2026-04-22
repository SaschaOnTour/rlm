//! Aggregated test-impact analysis for a write operation (task #105 / T4).
//!
//! Composes the T2 discovery strategies from [`super::test_impact`]
//! (`find_direct_tests`, `find_transitive_tests`, `find_tests_by_naming`)
//! with the T3 runner detection (`detect_runner`, `generate_test_command`
//! from [`super::test_runner`]) into the shape that gets spliced into
//! every write-response JSON.
//!
//! Split out of `test_impact.rs` during T4 when the file crossed the
//! SRP_MODULE threshold: T1/T2 primitives + strategies live there,
//! T4 aggregation lives here.

use crate::application::symbol::test_impact::{
    find_direct_tests, find_tests_by_naming, find_transitive_tests, TestMatch,
};
use crate::application::symbol::test_runner::{detect_runner, generate_test_command};
use crate::db::Database;
use crate::error::Result;

/// Wire format of the `test_impact` block in a write-response JSON.
///
/// Serialization skips empty fields so the envelope degrades cleanly:
/// no tests → only `no_tests_warning`; tests but no detected runner
/// → only `run_tests`; nothing applicable → the helper returns
/// `None` and the caller omits the whole field.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TestImpactResult {
    /// Flattened list of test-symbol names, in strategy priority
    /// (Direct > Transitive > NamingConvention), de-duplicated.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub run_tests: Vec<String>,
    /// Shell command that runs exactly `run_tests`. `None` when the
    /// runner isn't detected or there are no tests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_command: Option<String>,
    /// Human-readable warning emitted when the changed symbol has no
    /// tests at all. Agents are expected to write one before
    /// shipping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_tests_warning: Option<String>,
    /// Symbols elsewhere in the codebase with names close to the
    /// changed one. Surfaced so the agent can decide whether a
    /// parallel change or a typo investigation is warranted. Empty
    /// when no near-neighbours exist.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub similar_symbols: Vec<crate::application::symbol::similar_symbols::SimilarSymbol>,
}

/// Analyze which tests cover a symbol that was just modified.
///
/// Walks the T2 strategies in priority order (Direct → Transitive →
/// NamingConvention), deduplicates by `(test_symbol, file)`, renders
/// the runner command via T3 when a runner is detected, and emits a
/// `no_tests_warning` when the result set is empty.
///
/// DB errors bubble up via `Result`. An *empty* analysis still
/// returns `Ok` with `no_tests_warning` populated — callers need
/// that signal.
// qual:api
pub fn analyze_test_impact(
    db: &Database,
    project_root: &std::path::Path,
    symbol: &str,
    changed_file: &str,
) -> Result<TestImpactResult> {
    let (merged, confirmed_count) = discover_tests(db, symbol, changed_file)?;
    let run_tests: Vec<String> = merged.iter().map(|m| m.test_symbol.clone()).collect();

    let file = db.get_file_by_path(changed_file)?;
    let lang = file.as_ref().map(|f| f.lang.clone()).unwrap_or_default();
    let runner = detect_runner(&lang, project_root);
    let test_command = runner.and_then(|r| generate_test_command(r, &merged));

    // Warning fires when *confirmed* coverage (Direct ∪ Transitive) is
    // empty. NamingConvention is heuristic — useful as candidates but
    // not proof that the symbol is actually tested.
    let no_tests_warning = build_warning(symbol, confirmed_count, run_tests.len());

    // Similar symbols elsewhere in the codebase: helps the agent
    // spot typos (`opne` → `open`) and naming-convention cousins
    // that may need a parallel change.
    let similar_symbols = crate::application::symbol::similar_symbols::find_similar_symbols(
        db,
        symbol,
        file.as_ref().map(|f| f.id),
        crate::application::symbol::similar_symbols::DEFAULT_MAX_DISTANCE,
        crate::application::symbol::similar_symbols::DEFAULT_TOP_N,
    )
    .unwrap_or_default();

    Ok(TestImpactResult {
        run_tests,
        test_command,
        no_tests_warning,
        similar_symbols,
    })
}

/// Decide whether the response should carry a warning, and what to
/// say. Distinguishes "no tests at all" from "only speculative
/// naming-convention candidates — you still need a Direct test".
fn build_warning(symbol: &str, confirmed_count: usize, total_candidates: usize) -> Option<String> {
    if confirmed_count > 0 {
        return None;
    }
    if total_candidates == 0 {
        return Some(format!(
            "No tests cover `{symbol}`. Write one before shipping — rlm found no Direct, Transitive, or NamingConvention match."
        ));
    }
    Some(format!(
        "No Direct or Transitive test covers `{symbol}`. The listed tests are speculative naming-convention candidates — add a Direct test before shipping."
    ))
}

/// Run all three discovery strategies in priority order, merge their
/// results, and return `(merged, confirmed_count)` where
/// `confirmed_count` is the number of matches that came from Direct
/// or Transitive — the strategies backed by actual ref-graph evidence.
/// NamingConvention hits inflate the merged list but not the
/// confirmed count, so `analyze_test_impact` can distinguish real
/// coverage from speculative neighbors.
fn discover_tests(
    db: &Database,
    symbol: &str,
    changed_file: &str,
) -> Result<(Vec<TestMatch>, usize)> {
    let mut out: Vec<TestMatch> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut confirmed: usize = 0;

    // Direct > Transitive > NamingConvention (first-seen wins on collisions).
    let direct = find_direct_tests(db, symbol, changed_file)?;
    let transitive = find_transitive_tests(db, symbol)?;
    let naming = find_tests_by_naming(db, changed_file)?;

    for m in direct.into_iter().chain(transitive) {
        let key = (m.test_symbol.clone(), m.file.clone());
        if seen.insert(key) {
            confirmed += 1;
            out.push(m);
        }
    }
    for m in naming {
        let key = (m.test_symbol.clone(), m.file.clone());
        if seen.insert(key) {
            out.push(m);
        }
    }

    Ok((out, confirmed))
}
