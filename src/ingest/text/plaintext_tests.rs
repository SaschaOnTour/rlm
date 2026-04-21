//! Tests for `plaintext.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "plaintext_tests.rs"] mod tests;`.

use super::{ChunkKind, PlaintextParser, TextParser};
fn parser() -> PlaintextParser {
    PlaintextParser::new()
}

#[test]
fn parse_plaintext_content() {
    let source = "key = value\nother = stuff\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].ident, "(document)");
    assert_eq!(chunks[0].kind, ChunkKind::Section);
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[0].end_line, 2);
    assert_eq!(chunks[0].content, source);
}

#[test]
fn parse_empty_plaintext() {
    let chunks = parser().parse_chunks("", 1).unwrap();
    assert!(chunks.is_empty());
}

#[test]
fn parse_whitespace_only() {
    let chunks = parser().parse_chunks("   \n  \n  ", 1).unwrap();
    assert!(chunks.is_empty());
}

#[test]
fn format_returns_plaintext() {
    assert_eq!(parser().format(), "plaintext");
}
