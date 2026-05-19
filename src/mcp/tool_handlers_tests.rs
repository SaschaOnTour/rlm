//! Tests for `tool_handlers.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "tool_handlers_tests.rs"] mod tests;`.

use super::handle_insert;
use crate::application::edit::inserter::InsertPosition;
use crate::application::session::RlmSession;
use crate::mcp::tool_handlers_util::handle_quality_clear;
use crate::output::Formatter;

#[test]
fn insert_with_relative_path_resolves_to_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    std::fs::write(&file_path, "fn main() {}\n").unwrap();

    // Index once so the facade can open the existing DB.
    RlmSession::index_project(dir.path(), None).unwrap();

    let result = handle_insert(
        dir.path(),
        &crate::mcp::tools::InsertParams {
            path: "test.rs".to_string(),
            position: InsertPosition::Top,
            code: "// header\n".to_string(),
        },
        Formatter::default(),
    );
    assert!(
        result.is_ok(),
        "insert should succeed with relative path + project_root"
    );

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(
        content.starts_with("// header"),
        "file should have inserted content at top"
    );
}

#[test]
fn insert_with_nonexistent_relative_path_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    // Build an empty index so the facade can open the session.
    RlmSession::index_project(dir.path(), None).unwrap();

    let result = handle_insert(
        dir.path(),
        &crate::mcp::tools::InsertParams {
            path: "nonexistent.rs".to_string(),
            position: InsertPosition::Top,
            code: "// hi\n".to_string(),
        },
        Formatter::default(),
    );
    let call_result = result.unwrap();
    assert_eq!(call_result.is_error, Some(true));
}

#[test]
fn handle_quality_clear_truncates_log_and_returns_cleared_ack() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();

    // Build an empty index so the session can open the project.
    RlmSession::index_project(dir.path(), None).unwrap();

    // Drop a non-empty payload into the quality log so we can verify
    // the truncate effect.
    let rlm_dir = dir.path().join(".rlm");
    std::fs::create_dir_all(&rlm_dir).unwrap();
    let log_path = rlm_dir.join("quality-issues.log");
    let mut f = std::fs::File::create(&log_path).unwrap();
    writeln!(
        f,
        r#"{{"file":"src/x.rs","lang":"rust","issue_type":"unknown","line":1}}"#
    )
    .unwrap();
    assert!(std::fs::metadata(&log_path).unwrap().len() > 0);

    let result = handle_quality_clear(dir.path(), Formatter::default());
    let call_result = result.unwrap();
    assert!(
        call_result.is_error != Some(true),
        "quality_clear must succeed when the log exists, got is_error={:?}",
        call_result.is_error,
    );

    let len_after = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
    assert_eq!(
        len_after, 0,
        "handle_quality_clear must leave the log empty, got {len_after} bytes",
    );
}
