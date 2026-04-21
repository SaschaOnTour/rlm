//! Tests for `ref_extractor.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "ref_extractor_tests.rs"] mod tests;`.

use super::{extract_references, Dispatcher};
#[test]
fn extract_rust_refs() {
    let d = Dispatcher::new();
    let source = r#"
fn helper() -> i32 { 42 }
fn main() {
    let x = helper();
}
"#;
    let chunks = d.parse("rust", source, 1).unwrap();
    let refs = extract_references(&d, "rust", source, &chunks).unwrap();
    assert!(refs.iter().any(|r| r.target_ident == "helper"));
}

#[test]
fn extract_markdown_refs_empty() {
    let d = Dispatcher::new();
    let chunks = d.parse("markdown", "# Title\nContent\n", 1).unwrap();
    let refs = extract_references(&d, "markdown", "# Title\nContent\n", &chunks).unwrap();
    assert!(refs.is_empty());
}
