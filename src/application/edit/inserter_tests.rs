//! Apply-insertion tests for `inserter.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "inserter_tests.rs"] mod tests;`.
//!
//! `InsertPosition` parsing / preview-source tests live in the sibling
//! `inserter_position_tests.rs`.

use super::{apply_insertion, insert_code, InsertPosition, SyntaxGuard};

/// Line number used to test out-of-bounds insertion.
const BEYOND_FILE_LINE: u32 = 10;

#[test]
fn insert_at_top() {
    let source = "line1\nline2\nline3";
    let result = apply_insertion(source, &InsertPosition::Top, "// header").unwrap();
    assert!(result.starts_with("// header"));
    assert!(result.contains("line1"));
}

#[test]
fn insert_at_bottom() {
    let source = "line1\nline2";
    let result = apply_insertion(source, &InsertPosition::Bottom, "// footer").unwrap();
    assert!(result.ends_with("// footer"));
}

#[test]
fn insert_before_line() {
    let source = "line1\nline2\nline3";
    let result = apply_insertion(source, &InsertPosition::BeforeLine(2), "// inserted").unwrap();
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines[1], "// inserted");
    assert_eq!(lines[2], "line2");
}

#[test]
fn insert_after_line() {
    let source = "line1\nline2\nline3";
    let result = apply_insertion(source, &InsertPosition::AfterLine(1), "// inserted").unwrap();
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines[0], "line1");
    assert_eq!(lines[1], "// inserted");
    assert_eq!(lines[2], "line2");
}

#[test]
fn insert_beyond_file_errors() {
    let source = "line1\nline2";
    let result = apply_insertion(
        source,
        &InsertPosition::AfterLine(BEYOND_FILE_LINE),
        "// nope",
    );
    assert!(result.is_err());
}

#[test]
fn insert_rejects_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    let guard = SyntaxGuard::new();
    let result = insert_code(dir.path(), "/etc/passwd", &InsertPosition::Top, "x", &guard);
    assert!(result.is_err());
    assert!(
        format!("{}", result.unwrap_err()).contains("path traversal"),
        "should reject absolute path"
    );
}

#[test]
fn insert_rejects_parent_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let guard = SyntaxGuard::new();
    let result = insert_code(
        dir.path(),
        "../etc/passwd",
        &InsertPosition::Top,
        "x",
        &guard,
    );
    assert!(result.is_err());
    assert!(
        format!("{}", result.unwrap_err()).contains("path traversal"),
        "should reject .. traversal"
    );
}
