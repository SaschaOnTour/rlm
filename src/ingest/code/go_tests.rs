//! Parser tests for `go.rs`.
//!
//! Moved out of `go.rs` in slice 4.9 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "go_tests.rs"] mod tests;`.

use super::GoParser;
use crate::domain::chunk::{ChunkKind, RefKind};
use crate::ingest::code::CodeParser;

fn parser() -> GoParser {
    GoParser::create()
}

#[test]
fn parse_go_function() {
    let source = r#"
package main

func Hello(name string) string {
    return "Hello, " + name
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "Hello").unwrap();
    assert_eq!(f.kind, ChunkKind::Function);
    assert_eq!(f.visibility.as_deref(), Some("pub"));
}

#[test]
fn parse_go_private_function() {
    let source = r#"
package main

func helper() int {
    return 42
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "helper").unwrap();
    assert_eq!(f.visibility.as_deref(), Some("private"));
}

#[test]
fn validate_syntax_valid_go() {
    assert!(parser().validate_syntax("package main\nfunc main() {}"));
}

#[test]
fn test_go_imports_extracted() {
    let source = r#"
package main

import (
    "fmt"
    "os"
    alias "path/filepath"
)

func main() {
    fmt.Println("hello")
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let imports_chunk = chunks.iter().find(|c| c.ident == "_imports");
    assert!(imports_chunk.is_some(), "Should have an _imports chunk");

    let refs = parser().extract_refs(source, &chunks).unwrap();
    let import_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.ref_kind == RefKind::Import)
        .collect();

    assert!(
        import_refs.len() >= 3,
        "Should capture at least 3 import refs (fmt, os, filepath), got {}",
        import_refs.len()
    );
}

#[test]
fn test_go_type_has_signature() {
    let source = r#"
package main

type Config struct {
    Name string
    Port int
}

type Handler interface {
    Handle() error
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let config = chunks.iter().find(|c| c.ident == "Config").unwrap();
    assert!(config.signature.is_some(), "Config should have a signature");
    assert!(
        config
            .signature
            .as_ref()
            .unwrap()
            .contains("type Config struct"),
        "Config signature should contain 'type Config struct', got: {:?}",
        config.signature
    );

    let handler = chunks.iter().find(|c| c.ident == "Handler").unwrap();
    assert!(
        handler.signature.is_some(),
        "Handler should have a signature"
    );
}
