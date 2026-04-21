//! Tests for `supported.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "supported_tests.rs"] mod tests;`.

use super::list_supported;
#[test]
fn list_supported_not_empty() {
    let result = list_supported();
    assert!(!result.extensions.is_empty());
}

#[test]
fn list_supported_includes_rust() {
    let result = list_supported();
    let rust = result.extensions.iter().find(|e| e.ext == ".rs");
    assert!(rust.is_some());
    let rust = rust.unwrap();
    assert_eq!(rust.lang, "rust");
    assert_eq!(rust.parser, "tree-sitter");
}

#[test]
fn list_supported_sorted() {
    let result = list_supported();
    let exts: Vec<&str> = result.extensions.iter().map(|e| e.ext.as_str()).collect();
    let mut sorted = exts.clone();
    sorted.sort();
    assert_eq!(exts, sorted);
}
