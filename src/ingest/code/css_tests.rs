//! Parser tests for `css.rs`.
//!
//! Moved out of `css.rs` in slice 4.9 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "css_tests.rs"] mod tests;`.

use super::*;
use crate::ingest::code::CodeParser;
use crate::models::chunk::ChunkKind;

fn parser() -> CssParser {
    CssParser::create()
}

#[test]
fn parse_css_rules() {
    let source = r#"
.container {
    max-width: 1200px;
    margin: 0 auto;
}

#header {
    background: white;
}

body {
    margin: 0;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == ".container"));
    assert!(chunks.iter().any(|c| c.ident == "#header"));
    assert!(chunks.iter().any(|c| c.ident == "body"));
}

#[test]
fn parse_css_media_queries() {
    let source = r#"
@media (min-width: 768px) {
    .container {
        width: 750px;
    }
}

@media screen and (max-width: 480px) {
    .mobile-only {
        display: block;
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let media_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c.kind == ChunkKind::Other("media".into()))
        .collect();
    assert!(
        media_chunks.len() >= 2,
        "Should find 2 media queries, got {:?}",
        media_chunks
    );
}

#[test]
fn parse_css_keyframes() {
    let source = r#"
@keyframes fadeIn {
    from {
        opacity: 0;
    }
    to {
        opacity: 1;
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "fadeIn"));
}

#[test]
fn extract_css_refs() {
    let source = r#"
.header .nav-item {
    color: blue;
}

#main .content {
    padding: 20px;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let refs = parser().extract_refs(source, &chunks).unwrap();

    // Should find class references
    let class_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.target_ident == "header" || r.target_ident == "nav-item")
        .collect();
    assert!(!class_refs.is_empty(), "Should find class references");
}

#[test]
fn css_variables() {
    let source = r#"
:root {
    --primary-color: #007bff;
    --secondary-color: #6c757d;
}

.button {
    background: var(--primary-color);
    color: white;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == ":root"));
    assert!(chunks.iter().any(|c| c.ident == ".button"));
    // CSS variable references require more complex parsing not currently supported
}

#[test]
fn validate_css_syntax() {
    assert!(parser().validate_syntax(".test { color: red; }"));
}

#[test]
fn byte_offset_round_trip() {
    let source = r#"
.container {
    width: 100%;
}

#header {
    background: blue;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    for chunk in &chunks {
        if chunk.ident == "_imports" {
            continue;
        }
        let reconstructed = &source[chunk.start_byte as usize..chunk.end_byte as usize];
        assert_eq!(
            reconstructed, chunk.content,
            "Byte offset reconstruction failed for chunk '{}'",
            chunk.ident
        );
    }
}

#[test]
fn empty_file() {
    let chunks = parser().parse_chunks("", 1).unwrap();
    assert!(chunks.is_empty());
}

#[test]
fn parse_with_quality_clean() {
    let source = ".test { color: red; }";
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(result.quality.is_complete());
}

#[test]
fn css_nesting() {
    // Modern CSS nesting (2023 spec)
    let source = r#"
.card {
    padding: 20px;

    & .title {
        font-size: 18px;
    }

    &:hover {
        background: gray;
    }
}
"#;
    // This may or may not work depending on tree-sitter-css support
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == ".card"));
}
