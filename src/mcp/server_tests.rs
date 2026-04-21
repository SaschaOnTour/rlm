//! Tests for `server.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "server_tests.rs"] mod tests;`.

use tempfile::TempDir;

/// Search result limit for test queries.
const TEST_SEARCH_LIMIT: usize = 10;

/// Setup: create temp dir with test file and index it
fn setup_indexed_project() -> (TempDir, crate::config::Config, crate::db::Database) {
    let tmp = TempDir::new().expect("create tempdir");

    std::fs::write(
        tmp.path().join("test.rs"),
        r#"/// A test struct for configuration.
pub struct Config {
    pub name: String,
    pub value: i32,
}

impl Config {
    pub fn new(name: String, value: i32) -> Self {
        Self { name, value }
    }
}

/// Helper function that doubles the input.
pub fn helper(x: i32) -> i32 {
    x * 2
}

fn internal() {
    let _cfg = Config::new("test".into(), 42);
    let _result = helper(10);
}
"#,
    )
    .expect("write test file");

    let config = crate::config::Config::new(tmp.path());
    crate::application::index::run_index(&config, None).expect("index project");
    let db = crate::db::Database::open(&config.db_path).expect("open db");

    (tmp, config, db)
}

#[test]
fn test_stats_operation_returns_expected_format() {
    let (_tmp, _config, db) = setup_indexed_project();
    let result = crate::operations::get_stats(&db).expect("get stats");
    assert!(result.files > 0);
    assert!(result.chunks > 0);
}

#[test]
fn test_search_operation_returns_results() {
    let (_tmp, _config, db) = setup_indexed_project();
    let result =
        crate::operations::search_chunks(&db, "helper", TEST_SEARCH_LIMIT).expect("search");
    assert!(!result.results.is_empty());
}

#[test]
fn test_refs_operation_returns_results() {
    let (_tmp, _config, db) = setup_indexed_project();
    let result = crate::operations::analyze_impact(&db, "helper").expect("refs/impact");
    assert!(result.count > 0);
}

#[test]
fn test_context_operation_returns_results() {
    let (_tmp, _config, db) = setup_indexed_project();
    let result = crate::operations::build_context(&db, "helper").expect("context");
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("helper"));
}

#[test]
fn test_overview_minimal_operation() {
    use crate::application::query::peek;
    let (_tmp, _config, db) = setup_indexed_project();
    let result = peek::peek(&db, None).expect("peek");
    assert!(!result.files.is_empty());
}

#[test]
fn test_overview_standard_operation() {
    let (_tmp, _config, db) = setup_indexed_project();
    let result = crate::operations::build_map(&db, None).expect("map");
    assert!(!result.results.is_empty());
}

#[test]
fn test_overview_tree_operation() {
    let (_tmp, _config, db) = setup_indexed_project();
    let result = crate::application::query::tree::build_tree(&db, None).expect("tree");
    assert!(!result.results.is_empty());
}

#[test]
fn test_callgraph_in_context_graph() {
    let (_tmp, _config, db) = setup_indexed_project();
    let _ctx = crate::operations::build_context(&db, "helper").expect("context");
    let _graph = crate::operations::build_callgraph(&db, "helper").expect("callgraph");
}
