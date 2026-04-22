//! Reindex / preview tests for `application::index::mod`.
//!
//! Split out of `mod_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). The basic index / incremental
//! reindex tests stay in `mod_tests.rs`; this file covers the preview
//! helpers and the `reindex_with_result` JSON envelope.

use super::fixtures::setup_indexed;
use super::run_index;
use super::{
    find_preview, reindex_with_result, resolve_test_impact_target, snapshot_chunk_keys, Config,
    PreviewSource, PREVIEW_LINES,
};
use std::fs;
use tempfile::TempDir;

/// Line inside the `helper` function in SAMPLE_SOURCE.
const HELPER_LINE: u32 = 6;
/// Line far beyond any file — used to test "not found" case.
const NONEXISTENT_LINE: u32 = 999;
/// Number of lines in the long function test (must exceed PREVIEW_LINES).
const LONG_FN_LINES: usize = 20;

const SAMPLE_SOURCE: &str = "\
fn main() {
println!(\"hello\");
}

fn helper(x: i32) -> i32 {
x * 2
}

fn another() -> bool {
true
}
";

#[test]
fn preview_symbol_returns_matching_chunk() {
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    let preview = find_preview(&db, "src/main.rs", &PreviewSource::Symbol("helper"));
    assert!(preview.is_some());
    let p = preview.unwrap();
    assert!(p.contains("helper"));
    assert!(p.contains("x * 2"));
}

#[test]
fn preview_symbol_not_found_returns_none() {
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    let preview = find_preview(&db, "src/main.rs", &PreviewSource::Symbol("nonexistent"));
    assert!(preview.is_none());
}

#[test]
fn preview_symbol_wrong_file_returns_none() {
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    let preview = find_preview(&db, "src/other.rs", &PreviewSource::Symbol("helper"));
    assert!(preview.is_none());
}

#[test]
fn preview_line_returns_containing_chunk() {
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    // helper is at lines 5-7, so line 6 should find it
    let preview = find_preview(&db, "src/main.rs", &PreviewSource::Line(HELPER_LINE));
    assert!(preview.is_some());
    let p = preview.unwrap();
    assert!(p.contains("helper"));
}

#[test]
fn preview_line_outside_chunks_returns_none() {
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    // Line 999 doesn't exist in any chunk
    let preview = find_preview(&db, "src/main.rs", &PreviewSource::Line(NONEXISTENT_LINE));
    assert!(preview.is_none());
}

#[test]
fn preview_none_returns_none() {
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    let preview = find_preview(&db, "src/main.rs", &PreviewSource::None);
    assert!(preview.is_none());
}

#[test]
fn preview_last_returns_last_chunk() {
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    let preview = find_preview(&db, "src/main.rs", &PreviewSource::Last);
    assert!(preview.is_some());
    // SAMPLE_SOURCE has main, helper, another — "another" is the last chunk
    let p = preview.unwrap();
    assert!(p.contains("another"));
}

#[test]
fn preview_truncates_long_chunks() {
    let long_fn = (0..LONG_FN_LINES)
        .map(|i| format!("    let x{i} = {i};"))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!("fn long_func() {{\n{long_fn}\n}}\n");
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", &source)]);
    let preview = find_preview(&db, "src/main.rs", &PreviewSource::Symbol("long_func"));
    assert!(preview.is_some());
    let p = preview.unwrap();
    let line_count = p.lines().count();
    assert_eq!(line_count, PREVIEW_LINES);
}

#[test]
fn reindex_with_result_includes_preview_for_symbol() {
    let (_tmp, config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    let json = reindex_with_result(&db, &config, "src/main.rs", PreviewSource::Symbol("helper"));
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["ok"], true);
    assert!(val["preview"].is_string());
    assert!(val["preview"].as_str().unwrap().contains("helper"));
}

#[test]
fn reindex_with_result_no_preview_for_none() {
    let (_tmp, config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    let json = reindex_with_result(&db, &config, "src/main.rs", PreviewSource::None);
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["ok"], true);
    assert!(val["preview"].is_null());
}

#[test]
fn reindex_with_result_includes_preview_for_line() {
    let (_tmp, config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    // Line 6 is inside helper
    let json = reindex_with_result(
        &db,
        &config,
        "src/main.rs",
        PreviewSource::Line(HELPER_LINE),
    );
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["ok"], true);
    assert!(val["preview"].is_string());
    assert!(val["preview"].as_str().unwrap().contains("helper"));
}

#[test]
fn run_index_calls_progress_callback() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.rs"), "fn a() {}").unwrap();
    fs::write(src.join("b.rs"), "fn b() {}").unwrap();

    let config = Config::new(tmp.path());
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let calls_clone = calls.clone();
    let progress = move |current: usize, total: usize| {
        calls_clone.lock().unwrap().push((current, total));
    };

    run_index(&config, Some(&progress)).unwrap();

    let recorded = calls.lock().unwrap();
    assert!(
        recorded.len() >= 2,
        "should be called at least once per file"
    );
    let &(last_current, last_total) = recorded.last().unwrap();
    assert_eq!(last_current, last_total, "last call should be total/total");
}

/// Same-ident methods under different `impl` blocks must hash to
/// different keys, otherwise the second `new` method looks like the
/// first when diffing post-reindex chunks and test-impact is missed.
#[test]
fn snapshot_chunk_keys_distinguishes_same_ident_under_different_parent() {
    const SRC: &str = "\
struct Foo;
struct Bar;

impl Foo {
    fn new() -> Self { Foo }
}

impl Bar {
    fn new() -> Self { Bar }
}
";
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SRC)]);
    let keys = snapshot_chunk_keys(&db, "src/main.rs");
    let news: Vec<_> = keys.iter().filter(|(_, ident, _)| ident == "new").collect();
    assert_eq!(
        news.len(),
        2,
        "both Foo::new and Bar::new must land in the snapshot as distinct keys, got {news:?}"
    );
    let parents: std::collections::HashSet<&Option<String>> =
        news.iter().map(|(_, _, p)| p).collect();
    assert_eq!(
        parents.len(),
        2,
        "the two `new` methods must differ by parent — otherwise the diff won't detect inserts under a new parent"
    );
}

/// A Symbol-preview source short-circuits the diff — the named symbol
/// is returned verbatim, regardless of what the snapshot looks like.
/// Pins the contract that `resolve_test_impact_target` is diff-only
/// for Line / Last preview sources.
#[test]
fn resolve_test_impact_target_respects_symbol_preview_source() {
    let (_tmp, _config, db) = setup_indexed(&[("main.rs", SAMPLE_SOURCE)]);
    let pre_keys = snapshot_chunk_keys(&db, "src/main.rs");
    let target = resolve_test_impact_target(
        &db,
        "src/main.rs",
        &PreviewSource::Symbol("helper"),
        &pre_keys,
    );
    assert_eq!(target.as_deref(), Some("helper"));
}
