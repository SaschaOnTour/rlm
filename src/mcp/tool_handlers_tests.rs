//! Tests for `tool_handlers.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "tool_handlers_tests.rs"] mod tests;`.

use super::handle_insert;
use crate::application::edit::inserter::InsertPosition;
use crate::application::session::RlmSession;
use crate::output::Formatter;

#[test]
fn insert_with_relative_path_resolves_to_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    std::fs::write(&file_path, "fn main() {}\n").unwrap();

    // Index once so the session can open an existing DB.
    RlmSession::index_project(dir.path(), None).unwrap();
    let session = RlmSession::open(dir.path()).unwrap();

    let result = handle_insert(
        Some(&session),
        &crate::mcp::tool_handlers::InsertInput {
            path: "test.rs",
            position: &InsertPosition::Top,
            code: "// header\n",
        },
        dir.path(),
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
    // Build an empty index so try_open_existing yields a session.
    RlmSession::index_project(dir.path(), None).unwrap();
    let session = RlmSession::open(dir.path()).unwrap();

    let result = handle_insert(
        Some(&session),
        &crate::mcp::tool_handlers::InsertInput {
            path: "nonexistent.rs",
            position: &InsertPosition::Top,
            code: "// hi\n",
        },
        dir.path(),
        Formatter::default(),
    );
    let call_result = result.unwrap();
    assert_eq!(call_result.is_error, Some(true));
}
