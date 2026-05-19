//! Tests for `server_helpers.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "server_helpers_tests.rs"] mod tests;`.

use super::{guard_output, Formatter, RlmServer, MAX_MCP_OUTPUT_BYTES};
use crate::config::Config;
#[test]
fn error_text_sets_is_error_true() {
    let result = RlmServer::error_text(Formatter::default(), "something failed".into());
    assert_eq!(result.is_error, Some(true));
}

#[test]
fn success_text_does_not_set_is_error() {
    let result = RlmServer::success_text(Formatter::default(), "ok".into());
    assert_ne!(result.is_error, Some(true));
}

#[test]
fn error_text_contains_message() {
    let result = RlmServer::error_text(Formatter::default(), "disk full".into());
    let text = result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .unwrap_or_default();
    assert!(text.contains("disk full"));
}

#[test]
fn guard_output_passes_small_result() {
    let small = "{\"ok\":true}".to_string();
    let result = guard_output(small.clone());
    assert_eq!(result, small);
}

#[test]
fn guard_output_truncates_large_result() {
    let large = "x".repeat(MAX_MCP_OUTPUT_BYTES + 1);
    let result = guard_output(large);
    assert!(result.contains("\"truncated\":true"));
    assert!(result.len() < MAX_MCP_OUTPUT_BYTES);
}

#[test]
fn mcp_facade_path_runs_staleness_check() {
    // Each MCP tool call funnels through `facades::*_project`, which
    // opens a fresh `RlmSession` for the server's project root.
    // `RlmSession::open` MUST invoke the self-healing staleness
    // check before returning — otherwise externally-modified files
    // stay invisible to tools and the MCP client serves stale data.
    //
    // Probed through an index-backed query (FTS search) so the
    // assertion actually depends on the DB being reconciled — a
    // filesystem scan like `list_files` would find externally-added
    // files even if staleness never ran and silently mask the bug.
    use crate::application::facades;
    use crate::application::query::search::FieldsMode;
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.rs"), "fn original() {}").unwrap();

    // Index once so the DB exists (and contains only `original`).
    let config = Config::new(tmp.path());
    crate::application::index::run_index(&config, None).unwrap();

    // Add a new symbol externally (not via rlm) — index now stale.
    fs::write(
        tmp.path().join("new.rs"),
        "fn externally_added_unique_marker() {}",
    )
    .unwrap();

    // MCP path: the search facade opens a fresh session, which must
    // reconcile staleness before serving the query.
    let response = facades::search_project(
        tmp.path(),
        "externally_added_unique_marker",
        10,
        FieldsMode::Full,
    )
    .expect("facade search succeeds");

    // DB-backed probe: FTS over the chunks table. If staleness never
    // ran, the `externally_added_unique_marker` symbol is not indexed
    // and the search comes back empty.
    let parsed: serde_json::Value =
        serde_json::from_str(&response.body).expect("search body is valid JSON");
    let results = parsed["results"].as_array().expect("`results` is an array");
    let names: Vec<&str> = results.iter().filter_map(|r| r["name"].as_str()).collect();

    assert!(
        names.contains(&"externally_added_unique_marker"),
        "MCP facade path must reconcile the index before serving \
         (FTS search found no hits for externally-added symbol — \
         staleness refresh not invoked). Names: {names:?}"
    );
}

#[test]
fn guard_output_boundary() {
    let exact = "x".repeat(MAX_MCP_OUTPUT_BYTES);
    let result = guard_output(exact.clone());
    assert_eq!(result, exact);
}
