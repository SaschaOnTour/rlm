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
fn ensure_session_runs_staleness_check_on_mcp_path() {
    // Regression test: the MCP canonical session-open (RlmServer::ensure_session)
    // must invoke the self-healing staleness check, mirroring the CLI session
    // open. This guards against accidentally losing the wiring from P07-05.
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.rs"), "fn original() {}").unwrap();

    // Index once so the DB exists.
    let config = Config::new(tmp.path());
    crate::application::index::run_index(&config, None).unwrap();

    // Add a new symbol externally (not via rlm) — index now stale.
    fs::write(tmp.path().join("new.rs"), "fn externally_added() {}").unwrap();

    // MCP path: ensure_session should reconcile before returning. We
    // probe via the session's typed read: a newly-visible file should
    // resolve immediately after ensure_session completes.
    let server = RlmServer::new(tmp.path().to_path_buf(), Formatter::default());
    let session = server.ensure_session().expect("ensure_session succeeds");
    let files = session
        .files(crate::application::query::files::FilesFilter {
            path_prefix: None,
            skipped_only: false,
            indexed_only: true,
        })
        .expect("session.files succeeds");

    assert!(
        files.results.iter().any(|f| f.path == "new.rs"),
        "MCP ensure_session must pick up externally-added files"
    );
}

#[test]
fn guard_output_boundary() {
    let exact = "x".repeat(MAX_MCP_OUTPUT_BYTES);
    let result = guard_output(exact.clone());
    assert_eq!(result, exact);
}
