//! Tests for `pdf.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "pdf_tests.rs"] mod tests;`.

use super::{ChunkKind, PdfParser, TextParser};
fn parser() -> PdfParser {
    PdfParser::new()
}

#[test]
fn parse_single_page() {
    let source = "This is page one content.\nSecond line.";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].ident, "Page 1");
    assert_eq!(chunks[0].kind, ChunkKind::Page);
}

#[test]
fn parse_multiple_pages() {
    let source = "Page one content\x0CPage two content\x0CPage three content";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].ident, "Page 1");
    assert_eq!(chunks[1].ident, "Page 2");
    assert_eq!(chunks[2].ident, "Page 3");
}

#[test]
fn parse_empty_pages_skipped() {
    let source = "Content\x0C\x0CMore content";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 2);
}
