//! Test-impact analysis for write operations.
//!
//! Given a symbol that was just modified (via `replace` or `insert`), figure
//! out which tests cover it and what command will run them. The goal is to
//! embed that answer directly into every write response so an AI agent never
//! has to guess what to run after an edit.
//!
//! T1 shipped the `is_test_file` / `is_test_chunk` primitives. T2 adds the
//! three discovery strategies that map a changed symbol to covering tests:
//! [`find_direct_tests`] (same-file callers), [`find_transitive_tests`]
//! (BFS backward through the ref graph, max depth 3), and
//! [`find_tests_by_naming`] (test-files whose stem matches the source's).
//! Runner detection + test-command rendering live in the sibling module
//! [`super::test_runner`]; T4 will compose them into the public
//! `analyze_test_impact` entry point.

use std::collections::{HashSet, VecDeque};

use crate::db::Database;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::error::Result;

/// Maximum BFS depth for [`find_transitive_tests`]. A test three calls
/// away from the changed symbol is still a plausible coverage path
/// (wrapper → helper → internal); anything deeper is usually noise.
const TRANSITIVE_MAX_DEPTH: u32 = 3;

/// Which strategy matched the test.
///
/// When the same test is found by multiple strategies, `Direct` wins over
/// `Transitive` wins over `NamingConvention` — direct evidence of
/// coverage is strongest, naming convention is just a heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryStrategy {
    /// Test chunk in the same file references the changed symbol.
    Direct,
    /// Test chunk reached by walking callers backward up to
    /// `TRANSITIVE_MAX_DEPTH` levels.
    Transitive,
    /// Test file's stem matches the changed file's stem (e.g.
    /// `src/auth.rs` ↔ `tests/auth_tests.rs`).
    NamingConvention,
}

/// One test that rlm thinks covers the changed symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestMatch {
    pub test_symbol: String,
    pub file: String,
    pub strategy: DiscoveryStrategy,
}

/// Whether a file path looks like a test file for the given language.
///
/// Pattern matching is done on the project-relative, forward-slash path
/// (rlm normalises separators at index time).
// qual:api
#[must_use]
pub fn is_test_file(path: &str, lang: &str) -> bool {
    match lang {
        "rust" => is_rust_test_file(path),
        "java" => is_java_test_file(path),
        "python" => is_python_test_file(path),
        "javascript" | "typescript" => is_js_ts_test_file(path),
        "go" => path.ends_with("_test.go"),
        "csharp" => is_csharp_test_file(path),
        "php" => file_name(path).is_some_and(|n| has_stem_before_suffix(n, "Test.php")),
        _ => false,
    }
}

/// Whether a chunk looks like a test case (a single `#[test]` fn, a
/// `@Test`-annotated method, a `TestFoo` Go function, …) for the given
/// language.
///
/// JS / TS return `false`: the parser does not capture `it(...)` /
/// `describe(...)` as annotated functions, so chunk-level detection is not
/// meaningful. For those languages the caller should fall back on
/// [`is_test_file`].
// qual:api
#[must_use]
pub fn is_test_chunk(chunk: &Chunk, lang: &str) -> bool {
    match lang {
        "rust" => attrs_contain(chunk, "#[test]"),
        "java" => attrs_contain(chunk, "@Test"),
        "python" => is_python_test_chunk(chunk),
        "javascript" | "typescript" => false,
        "go" => matches!(chunk.kind, ChunkKind::Function) && chunk.ident.starts_with("Test"),
        "csharp" => is_csharp_test_chunk(chunk),
        "php" => chunk.ident.starts_with("test_") || attrs_contain(chunk, "#[Test]"),
        _ => false,
    }
}

// ─── Per-language file-pattern helpers ──────────────────────────────────

fn is_rust_test_file(path: &str) -> bool {
    path.starts_with("tests/") || path.ends_with("_tests.rs") || path.ends_with("_test.rs")
}

fn is_java_test_file(path: &str) -> bool {
    if path.starts_with("src/test/") {
        return true;
    }
    file_name(path).is_some_and(|n| {
        has_stem_before_suffix(n, "Test.java") || has_stem_before_suffix(n, "Tests.java")
    })
}

fn is_python_test_file(path: &str) -> bool {
    path.starts_with("tests/")
        || path.ends_with("_test.py")
        || file_name(path).is_some_and(|n| n.starts_with("test_") && n.ends_with(".py"))
}

fn is_js_ts_test_file(path: &str) -> bool {
    path.starts_with("__tests__/")
        || path.contains("/__tests__/")
        || file_name(path).is_some_and(is_js_ts_test_filename)
}

fn is_csharp_test_file(path: &str) -> bool {
    file_name(path).is_some_and(|n| {
        has_stem_before_suffix(n, "Tests.cs") || (n.contains(".Test.") && n.ends_with(".cs"))
    })
}

// ─── Per-language chunk-marker helpers ──────────────────────────────────

fn is_python_test_chunk(chunk: &Chunk) -> bool {
    chunk.ident.starts_with("test_")
        || chunk
            .attributes
            .as_deref()
            .is_some_and(|a| a.contains("@pytest") || a.contains("@unittest"))
}

fn is_csharp_test_chunk(chunk: &Chunk) -> bool {
    chunk.attributes.as_deref().is_some_and(|a| {
        a.contains("[Fact]")
            || a.contains("[Theory]")
            || a.contains("[Test]")
            || a.contains("[TestMethod]")
    })
}

// ─── Shared string helpers ──────────────────────────────────────────────

fn attrs_contain(chunk: &Chunk, needle: &str) -> bool {
    chunk
        .attributes
        .as_deref()
        .is_some_and(|a| a.contains(needle))
}

fn file_name(path: &str) -> Option<&str> {
    path.rsplit('/').next()
}

/// True if `name` ends with `suffix` AND has at least one character
/// before it. Used by the per-language file-matchers to avoid classifying
/// a bare `Test.java` / `Tests.cs` / `Test.php` (typical for scaffolding
/// base classes) as a concrete test-case file.
fn has_stem_before_suffix(name: &str, suffix: &str) -> bool {
    name.len() > suffix.len() && name.ends_with(suffix)
}

fn is_js_ts_test_filename(name: &str) -> bool {
    // Matches foo.test.ts, foo.test.js, foo.spec.tsx, foo.spec.jsx, etc.
    // The interior `.test.` / `.spec.` delimiter is enough — the trailing
    // extension check keeps regular files like `test.config.ts` from
    // matching (no `.test.` infix there).
    name.contains(".test.") || name.contains(".spec.")
}

// ─── Discovery strategies (T2) ──────────────────────────────────────────

/// Tests in the same file that directly reference the changed symbol.
///
/// Walks `get_refs_with_context` for the symbol, filters to hits whose
/// file matches `changed_file`, and returns the callers that pass
/// [`is_test_chunk`].
// qual:api
pub fn find_direct_tests(
    db: &Database,
    symbol: &str,
    changed_file: &str,
) -> Result<Vec<TestMatch>> {
    let refs = db.get_refs_with_context(symbol)?;
    let mut matches = Vec::new();
    let mut seen = HashSet::new();
    for r in refs {
        if r.file_path != changed_file {
            continue;
        }
        let Some(caller) = resolve_caller_chunk(db, &r.containing_symbol, &r.file_path)? else {
            continue;
        };
        let Some(lang) = file_lang(db, caller.file_id)? else {
            continue;
        };
        if !is_test_chunk(&caller, &lang) {
            continue;
        }
        if seen.insert((caller.ident.clone(), r.file_path.clone())) {
            matches.push(TestMatch {
                test_symbol: caller.ident,
                file: r.file_path,
                strategy: DiscoveryStrategy::Direct,
            });
        }
    }
    Ok(matches)
}

/// BFS backward through the caller graph, stopping each branch at the
/// first test chunk reached (or at [`TRANSITIVE_MAX_DEPTH`]).
///
/// Non-test callers at depth 1..N are traversed but not recorded;
/// their own callers are enqueued for the next level.
// qual:api
pub fn find_transitive_tests(db: &Database, symbol: &str) -> Result<Vec<TestMatch>> {
    let mut matches = Vec::new();
    let mut seen_targets: HashSet<String> = HashSet::new();
    let mut seen_matches: HashSet<(String, String)> = HashSet::new();
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();
    queue.push_back((symbol.to_string(), 0));
    seen_targets.insert(symbol.to_string());

    while let Some((target, depth)) = queue.pop_front() {
        if depth >= TRANSITIVE_MAX_DEPTH {
            continue;
        }
        let refs = db.get_refs_with_context(&target)?;
        for r in refs {
            let Some(caller) = resolve_caller_chunk(db, &r.containing_symbol, &r.file_path)? else {
                continue;
            };
            let Some(lang) = file_lang(db, caller.file_id)? else {
                continue;
            };
            if is_test_chunk(&caller, &lang) {
                // Record the test and stop this branch.
                if seen_matches.insert((caller.ident.clone(), r.file_path.clone())) {
                    matches.push(TestMatch {
                        test_symbol: caller.ident,
                        file: r.file_path,
                        strategy: DiscoveryStrategy::Transitive,
                    });
                }
                continue;
            }
            // Non-test caller: enqueue for the next BFS level (guarded
            // against cycles by `seen_targets`).
            if seen_targets.insert(caller.ident.clone()) {
                queue.push_back((caller.ident, depth + 1));
            }
        }
    }
    Ok(matches)
}

/// Test files whose stem matches the changed file's stem (e.g.
/// `src/auth.rs` → `tests/auth_tests.rs`). Every test chunk in those
/// files is returned, regardless of whether it references the changed
/// symbol — the heuristic is "this test file is named after the source,
/// so its tests probably exercise it".
// qual:api
pub fn find_tests_by_naming(db: &Database, changed_file: &str) -> Result<Vec<TestMatch>> {
    let Some(source_file) = db.get_file_by_path(changed_file)? else {
        return Ok(Vec::new());
    };
    let Some(stem) = source_stem(&source_file.path) else {
        return Ok(Vec::new());
    };
    let all_files = db.get_all_files()?;
    let candidates = matching_test_files(&all_files, &source_file.lang, stem);
    collect_naming_matches(db, &candidates, &source_file.lang)
}

/// Pure filter — "of the candidate files, which are test files that
/// cover the source stem, in the same language?" Extracted so the
/// integration layer above stays call-only.
fn matching_test_files<'a>(
    all: &'a [crate::domain::file::FileRecord],
    source_lang: &str,
    source_stem: &str,
) -> Vec<&'a crate::domain::file::FileRecord> {
    all.iter()
        .filter(|f| f.lang == source_lang)
        .filter(|f| is_test_file(&f.path, &f.lang))
        .filter(|f| test_file_covers_source(&f.path, source_stem))
        .collect()
}

/// For each candidate file, pull its chunks and keep the ones that pass
/// `is_test_chunk`. Integration-only: the filtering logic lives in
/// [`matching_test_files`] and [`is_test_chunk`].
fn collect_naming_matches(
    db: &Database,
    candidates: &[&crate::domain::file::FileRecord],
    lang: &str,
) -> Result<Vec<TestMatch>> {
    let mut matches = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for f in candidates {
        for chunk in db.get_chunks_for_file(f.id)? {
            if is_test_chunk(&chunk, lang) && seen.insert((chunk.ident.clone(), f.path.clone())) {
                matches.push(TestMatch {
                    test_symbol: chunk.ident,
                    file: f.path.clone(),
                    strategy: DiscoveryStrategy::NamingConvention,
                });
            }
        }
    }
    Ok(matches)
}

// ─── Strategy-support helpers ───────────────────────────────────────────

/// Find the [`Chunk`] record for a caller identified by its ident + file
/// path. `get_refs_with_context` gives us the caller's symbol name and
/// the file; we look up the chunk in that file.
fn resolve_caller_chunk(
    db: &Database,
    caller_ident: &str,
    file_path: &str,
) -> Result<Option<Chunk>> {
    let Some(file) = db.get_file_by_path(file_path)? else {
        return Ok(None);
    };
    let chunks = db.get_chunks_by_ident(caller_ident)?;
    Ok(chunks.into_iter().find(|c| c.file_id == file.id))
}

/// Look up a file's language by numeric id.
fn file_lang(db: &Database, file_id: i64) -> Result<Option<String>> {
    // No direct by-id query on FileRepo; walk `get_all_files` once.
    // Callers invoke this at most once per caller chunk, which in
    // practice is <100 entries for realistic symbols.
    for f in db.get_all_files()? {
        if f.id == file_id {
            return Ok(Some(f.lang));
        }
    }
    Ok(None)
}

/// Extract the file stem (everything between the last `/` and the last `.`).
fn source_stem(path: &str) -> Option<&str> {
    let name = file_name(path)?;
    Some(name.rsplit_once('.').map_or(name, |(stem, _)| stem))
}

/// True if the test file's basename-stem "covers" `source_stem` — either
/// they match exactly, or the source stem is a prefix/suffix with a
/// sensible word boundary after/before it (`_`, `.`, or Pascal-case
/// transition).
fn test_file_covers_source(test_path: &str, source: &str) -> bool {
    let Some(test_stem) = source_stem(test_path) else {
        return false;
    };
    stem_matches(test_stem, source)
}

fn stem_matches(test_stem: &str, source_stem: &str) -> bool {
    if test_stem == source_stem {
        return true;
    }
    if source_stem.is_empty() || test_stem.len() <= source_stem.len() {
        return false;
    }
    if test_stem.starts_with(source_stem) {
        let next = test_stem.as_bytes()[source_stem.len()] as char;
        if is_stem_boundary_after(next, source_stem) {
            return true;
        }
    }
    if test_stem.ends_with(source_stem) {
        let prev_idx = test_stem.len() - source_stem.len() - 1;
        let prev = test_stem.as_bytes()[prev_idx] as char;
        let first = source_stem.chars().next().unwrap_or(' ');
        if is_stem_boundary_before(prev, first) {
            return true;
        }
    }
    false
}

fn is_stem_boundary_after(next: char, source_stem: &str) -> bool {
    if next == '_' || next == '.' {
        return true;
    }
    // PascalCase transition: source ended lowercase, next char uppercase.
    let last = source_stem.chars().last().unwrap_or(' ');
    last.is_ascii_lowercase() && next.is_ascii_uppercase()
}

fn is_stem_boundary_before(prev: char, first: char) -> bool {
    if prev == '_' || prev == '.' {
        return true;
    }
    // PascalCase transition: prev char lowercase, source starts uppercase.
    prev.is_ascii_lowercase() && first.is_ascii_uppercase()
}

#[cfg(test)]
#[path = "test_impact_tests.rs"]
mod tests;
