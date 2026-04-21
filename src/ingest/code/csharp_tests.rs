//! Parser tests for `csharp.rs`.
//!
//! Moved out of `csharp.rs` in slice 4.7 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "csharp_tests.rs"] mod tests;`.

use super::CSharpParser;
use crate::domain::chunk::{ChunkKind, RefKind};
use crate::ingest::code::CodeParser;

fn parser() -> CSharpParser {
    CSharpParser::create()
}

#[test]
fn parse_csharp_class() {
    let source = r#"
public class UserService {
    public string GetUser(int id) {
        return "user";
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "UserService" && c.kind == ChunkKind::Class));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "GetUser" && c.kind == ChunkKind::Method));
}

#[test]
fn validate_csharp_syntax() {
    assert!(parser().validate_syntax("public class Foo { public void Bar() {} }"));
}

#[test]
fn test_csharp_imports_extracted() {
    let source = r#"
using System;
using System.Collections.Generic;
using System.Linq;

public class Test {
    public void Test() {}
}
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
        import_refs.len() >= 2,
        "Should capture at least 2 import refs, got {}",
        import_refs.len()
    );
}

#[test]
fn test_csharp_no_duplicate_methods() {
    let source = r#"
public class UserService {
    public string GetUser(int id) {
        return "user";
    }

    public void SetUser(string name) {
        Console.WriteLine(name);
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    // Count method chunks
    let get_user_chunks: Vec<_> = chunks.iter().filter(|c| c.ident == "GetUser").collect();
    assert_eq!(
        get_user_chunks.len(),
        1,
        "Should have exactly 1 'GetUser' chunk, got {}",
        get_user_chunks.len()
    );

    let set_user_chunks: Vec<_> = chunks.iter().filter(|c| c.ident == "SetUser").collect();
    assert_eq!(
        set_user_chunks.len(),
        1,
        "Should have exactly 1 'SetUser' chunk, got {}",
        set_user_chunks.len()
    );
}

#[test]
fn test_csharp_class_has_signature() {
    let source = r#"
public class UserService : IUserService {
    public void Handle() {}
}

public interface IUserService {
    void Handle();
}

public struct Point {
    public int X;
    public int Y;
}
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
            .contains("public class UserService"),
        "UserService signature should contain class declaration, got: {:?}",
        user_service.signature
    );

    let iuser_service = chunks.iter().find(|c| c.ident == "IUserService").unwrap();
    assert!(
        iuser_service.signature.is_some(),
        "IUserService should have a signature"
    );

    let point = chunks.iter().find(|c| c.ident == "Point").unwrap();
    assert!(point.signature.is_some(), "Point should have a signature");
}

// ============================================================
// PHASE 2: Critical Reliability Tests
// ============================================================

/// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
#[test]
fn byte_offset_round_trip() {
    let source = r#"
public class Main {
    public static void Main(string[] args) {
        Console.WriteLine("Hello");
    }

    private int Helper(int x) {
        return x * 2;
    }
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
            "Byte offset reconstruction failed for chunk '{}'",
            chunk.ident
        );
    }
}

/// Unicode identifiers.
#[test]
fn unicode_identifiers() {
    let source = r#"
public class Größe {
    public int Berechne() {
        return 42;
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let groesse = chunks.iter().find(|c| c.ident == "Größe");
    assert!(groesse.is_some(), "Should find class with German umlaut");
}

/// CRLF line endings.
#[test]
fn crlf_line_endings() {
    let source =
        "public class Foo {\r\n    public void Bar() {\r\n        int x = 1;\r\n    }\r\n}\r\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let foo = chunks.iter().find(|c| c.ident == "Foo").unwrap();
    assert_eq!(foo.start_line, 1, "Start line should be 1");
    assert_eq!(foo.end_line, 5, "End line should account for CRLF");
}

/// Reference positions must be within their containing chunk.
#[test]
fn reference_positions_within_chunks() {
    let source = r#"
public class Service {
    public void Process() {
        Helper();
        OtherMethod();
    }

    private void Helper() {}
    private void OtherMethod() {}
}
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
