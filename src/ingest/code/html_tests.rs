//! Parser tests for `html.rs`.
//!
//! Moved out of `html.rs` in slice 4.9 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "html_tests.rs"] mod tests;`.

use super::*;
use crate::ingest::code::CodeParser;
use crate::models::chunk::RefKind;

fn parser() -> HtmlParser {
    HtmlParser::create()
}

#[test]
fn parse_html_with_ids() {
    let source = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Test</title>
</head>
<body>
    <div id="app">
        <header id="header">Header</header>
        <main id="content">Content</main>
        <footer id="footer">Footer</footer>
    </div>
</body>
</html>
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "app"));
    assert!(chunks.iter().any(|c| c.ident == "header"));
    assert!(chunks.iter().any(|c| c.ident == "content"));
    assert!(chunks.iter().any(|c| c.ident == "footer"));
}

#[test]
fn parse_html_script_style() {
    let source = r#"
<html>
<head>
    <style>
        body { margin: 0; }
    </style>
</head>
<body>
    <script>
        console.log("Hello");
    </script>
</body>
</html>
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "_script"));
    assert!(chunks.iter().any(|c| c.ident == "_style"));
}

#[test]
fn extract_html_refs() {
    let source = r#"
<div id="app" class="container main">
    <a href="/about">About</a>
    <img src="logo.png" class="logo">
</div>
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let refs = parser().extract_refs(source, &chunks).unwrap();

    // Should find class references
    let class_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.ref_kind == RefKind::TypeUse)
        .collect();
    assert!(
        class_refs.len() >= 3,
        "Should find at least 3 class references"
    );

    // Should find href/src imports
    let import_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.ref_kind == RefKind::Import)
        .collect();
    assert!(import_refs.len() >= 2, "Should find at least 2 import refs");
}

#[test]
fn validate_html_syntax() {
    assert!(parser().validate_syntax("<div>Hello</div>"));
}

#[test]
fn byte_offset_round_trip() {
    let source = r#"
<div id="app">
    <span id="inner">Text</span>
</div>
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    for chunk in &chunks {
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
fn html5_semantic_elements() {
    let source = r#"
<!DOCTYPE html>
<html>
<body>
    <header id="header">
        <nav id="nav">Navigation</nav>
    </header>
    <main id="main">
        <article id="post">
            <section id="intro">Introduction</section>
        </article>
        <aside id="sidebar">Sidebar</aside>
    </main>
    <footer id="footer">Footer</footer>
</body>
</html>
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "header"));
    assert!(chunks.iter().any(|c| c.ident == "nav"));
    assert!(chunks.iter().any(|c| c.ident == "main"));
    assert!(chunks.iter().any(|c| c.ident == "post"));
    assert!(chunks.iter().any(|c| c.ident == "sidebar"));
}

#[test]
fn parse_with_quality_clean() {
    let source = "<div>Hello</div>";
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(result.quality.is_complete());
}
