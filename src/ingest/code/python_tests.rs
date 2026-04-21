//! Parser tests for `python.rs`.
//!
//! Moved out of `python.rs` in slice 4.5 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "python_tests.rs"] mod tests;`.

use super::PythonParser;
use crate::domain::chunk::{ChunkKind, RefKind};
use crate::ingest::code::CodeParser;

fn parser() -> PythonParser {
    PythonParser::create()
}

#[test]
fn parse_python_function() {
    let source = "def hello(name: str) -> str:\n    return f'Hello, {name}'\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "hello").unwrap();
    assert_eq!(f.kind, ChunkKind::Function);
    assert_eq!(f.visibility.as_deref(), Some("public"));
}

#[test]
fn parse_python_class_with_methods() {
    let source = r#"
class UserService:
    def __init__(self, db):
        self.db = db

    def get_user(self, user_id):
        return self.db.find(user_id)

    def _internal(self):
        pass
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "UserService" && c.kind == ChunkKind::Class));
    let init = chunks.iter().find(|c| c.ident == "__init__").unwrap();
    assert_eq!(init.kind, ChunkKind::Method);
    assert_eq!(init.parent.as_deref(), Some("UserService"));
    assert_eq!(init.visibility.as_deref(), Some("dunder"));

    let internal = chunks.iter().find(|c| c.ident == "_internal").unwrap();
    assert_eq!(internal.visibility.as_deref(), Some("private"));
}

#[test]
fn validate_python_syntax() {
    assert!(parser().validate_syntax("def foo():\n    pass\n"));
}

#[test]
fn test_python_imports_extracted() {
    let source = r#"
import os
import sys
from datetime import datetime
from collections import defaultdict, OrderedDict
import json as j

def main():
    pass
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    // Verify _imports chunk exists
    let imports_chunk = chunks.iter().find(|c| c.ident == "_imports");
    assert!(imports_chunk.is_some(), "Should have an _imports chunk");

    // Verify refs extraction captures imports
    let refs = parser().extract_refs(source, &chunks).unwrap();
    let import_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.ref_kind == RefKind::Import)
        .collect();

    assert!(
        import_refs.len() >= 3,
        "Should capture at least 3 import refs, got {}",
        import_refs.len()
    );
}

#[test]
fn test_python_class_has_signature() {
    let source = r#"
class UserService(BaseService, Mixin):
    def __init__(self, db):
        self.db = db

class SimpleClass:
    pass
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let user_service = chunks.iter().find(|c| c.ident == "UserService").unwrap();
    assert!(
        user_service.signature.is_some(),
        "UserService should have a signature"
    );
    assert!(
        user_service
            .signature
            .as_ref()
            .unwrap()
            .contains("class UserService"),
        "UserService signature should contain class declaration, got: {:?}",
        user_service.signature
    );
    assert!(
        user_service
            .signature
            .as_ref()
            .unwrap()
            .contains("BaseService"),
        "UserService signature should contain base class, got: {:?}",
        user_service.signature
    );

    let simple_class = chunks.iter().find(|c| c.ident == "SimpleClass").unwrap();
    assert!(
        simple_class.signature.is_some(),
        "SimpleClass should have a signature"
    );
}

// ============================================================
// PHASE 2: Critical Reliability Tests
// ============================================================

/// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
#[test]
fn byte_offset_round_trip() {
    let source = r#"
def hello(name):
    return f"Hello, {name}"

class Config:
    def __init__(self, name):
        self.name = name
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
            "Byte offset reconstruction failed for chunk '{}'",
            chunk.ident
        );
    }
}

/// Unicode identifiers.
#[test]
fn unicode_identifiers() {
    let source = "def größe():\n    return 42\n\ndef 计算():\n    return 0\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let groesse = chunks.iter().find(|c| c.ident == "größe");
    assert!(groesse.is_some(), "Should find function with German umlaut");

    let chinese = chunks.iter().find(|c| c.ident == "计算");
    assert!(chinese.is_some(), "Should find function with Chinese name");
}

/// CRLF line endings.
#[test]
fn crlf_line_endings() {
    let source = "def foo():\r\n    return 42\r\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let f = chunks.iter().find(|c| c.ident == "foo").unwrap();
    assert_eq!(f.start_line, 1, "Start line should be 1");
    assert_eq!(f.end_line, 2, "End line should be 2 with CRLF");
}

/// Reference positions must be within their containing chunk.
#[test]
fn reference_positions_within_chunks() {
    let source = r#"
def caller():
    helper()
    other_fn()

def helper():
    return 42

def other_fn():
    return 0
"#;
    let (chunks, refs) = parser().parse_chunks_and_refs(source, 1).unwrap();

    for r in &refs {
        if r.chunk_id != 0 {
            if let Some(c) = chunks.iter().find(|c| c.id == r.chunk_id) {
                assert!(
                    r.line >= c.start_line && r.line <= c.end_line,
                    "Reference to '{}' at line {} should be within chunk '{}' lines {}-{}",
                    r.target_ident,
                    r.line,
                    c.ident,
                    c.start_line,
                    c.end_line
                );
            }
        }
    }
}
