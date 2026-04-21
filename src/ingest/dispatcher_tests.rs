//! Tests for `dispatcher.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "dispatcher_tests.rs"] mod tests;`.

use super::Dispatcher;
#[test]
fn dispatcher_supports_languages() {
    let d = Dispatcher::new();
    assert!(d.supports("rust"));
    assert!(d.supports("go"));
    assert!(d.supports("java"));
    assert!(d.supports("csharp"));
    assert!(d.supports("python"));
    assert!(d.supports("php"));
    assert!(d.supports("markdown"));
    assert!(d.supports("pdf"));
    assert!(!d.supports("haskell"));
}

#[test]
fn dispatcher_parses_rust() {
    let d = Dispatcher::new();
    let chunks = d.parse("rust", "fn main() {}", 1).unwrap();
    assert!(!chunks.is_empty());
}

#[test]
fn dispatcher_parses_markdown() {
    let d = Dispatcher::new();
    let chunks = d.parse("markdown", "# Hello\n\nContent\n", 1).unwrap();
    assert!(!chunks.is_empty());
}

#[test]
fn dispatcher_rejects_unknown() {
    let d = Dispatcher::new();
    assert!(d.parse("brainfuck", "+++", 1).is_err());
}

#[test]
fn dispatcher_validates_code() {
    let d = Dispatcher::new();
    assert!(d.validate_syntax("rust", "fn main() {}"));
    assert!(!d.validate_syntax("rust", "fn main() {"));
    assert!(d.validate_syntax("markdown", "anything"));
}
