//! Tests for `markdown.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "markdown_tests.rs"] mod tests;`.

use super::{MarkdownParser, TextParser};
fn parser() -> MarkdownParser {
    MarkdownParser::new()
}

#[test]
fn parse_headings() {
    let source =
        "# Title\n\nIntro text\n\n## Section 1\n\nContent 1\n\n## Section 2\n\nContent 2\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].ident, "Title");
    assert_eq!(chunks[1].ident, "Section 1");
    assert_eq!(chunks[1].parent.as_deref(), Some("Title"));
    assert_eq!(chunks[2].ident, "Section 2");
}

#[test]
fn parse_nested_headings() {
    let source = "# Top\n\n## Sub\n\n### Sub-sub\n\nContent\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[2].ident, "Sub-sub");
    assert_eq!(chunks[2].parent.as_deref(), Some("Sub"));
}

#[test]
fn parse_no_headings() {
    let source = "Just some text\nwithout any headings\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].ident, "(document)");
}

#[test]
fn parse_empty() {
    let chunks = parser().parse_chunks("", 1).unwrap();
    assert!(chunks.is_empty());
}
