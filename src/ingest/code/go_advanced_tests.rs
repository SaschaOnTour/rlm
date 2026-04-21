//! Advanced parser tests for `go.rs` (reliability / edge cases).
//!
//! Split out of `go_tests.rs` to keep each test companion focused on a
//! smaller cluster of behaviors (SRP_MODULE).

use super::GoParser;
use crate::ingest::code::CodeParser;

fn parser() -> GoParser {
    GoParser::create()
}

#[test]
fn byte_offset_round_trip() {
    let source = r#"
package main

func Hello(name string) string {
    return "Hello, " + name
}

type Config struct {
    Name string
    Port int
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty(), "Should have extracted chunks");

    for chunk in &chunks {
        if chunk.ident == "_imports" {
            continue;
        }
        let reconstructed = &source[chunk.start_byte as usize..chunk.end_byte as usize];
        assert_eq!(
            reconstructed, chunk.content,
            "Byte offset reconstruction failed for chunk '{}'\n\
             Expected bytes {}..{} to equal content",
            chunk.ident, chunk.start_byte, chunk.end_byte
        );
    }
}

#[test]
fn unicode_identifiers() {
    let source = r#"
package main

func größe() int {
    return 42
}

type Größe struct {
    Wert int
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let groesse = chunks.iter().find(|c| c.ident == "größe");
    assert!(groesse.is_some(), "Should find function with German umlaut");

    let struct_groesse = chunks.iter().find(|c| c.ident == "Größe");
    assert!(
        struct_groesse.is_some(),
        "Should find struct with German umlaut"
    );
}

#[test]
fn crlf_line_endings() {
    let source = "package main\r\n\r\nfunc foo() {\r\n    bar()\r\n}\r\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let f = chunks.iter().find(|c| c.ident == "foo").unwrap();
    assert_eq!(f.start_line, 3, "Start line should be 3");
    assert_eq!(f.end_line, 5, "End line should account for CRLF");
}
