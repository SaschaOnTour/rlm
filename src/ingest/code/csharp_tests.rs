//! Parser tests for `csharp.rs`.
//!
//! Moved out of `csharp.rs` in slice 4.7 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "csharp_tests.rs"] mod tests;`.

use super::*;
use crate::ingest::code::CodeParser;
use crate::models::chunk::{ChunkKind, RefKind};

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

// ============================================================
// PHASE 3: Modern Language Features
// ============================================================

/// C# records (9+) - likely NOT supported in tree-sitter-c-sharp 0.23.
#[test]
#[ignore = "C# records not supported in tree-sitter-c-sharp 0.23.1 (latest on crates.io)"]
fn csharp_records() {
    let source = r#"
public record Point(int X, int Y);

public record Person(string Name, int Age) {
    public string Greeting() {
        return $"Hello, {Name}";
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let point = chunks.iter().find(|c| c.ident == "Point");
    assert!(point.is_some(), "Should find record Point");
}

/// C# nullable reference types.
#[test]
fn csharp_nullable_types() {
    let source = r#"
public class Config {
    public string? Name { get; set; }
    public int? Port { get; set; }

    public string GetName() {
        return Name ?? "default";
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let config = chunks.iter().find(|c| c.ident == "Config").unwrap();
    assert_eq!(config.kind, ChunkKind::Class);
}

/// C# generics.
#[test]
fn csharp_generics() {
    let source = r#"
public class Repository<T> where T : class {
    public T GetById(int id) {
        return default;
    }

    public IEnumerable<T> GetAll() {
        return new List<T>();
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let repo = chunks.iter().find(|c| c.ident == "Repository").unwrap();
    assert_eq!(repo.kind, ChunkKind::Class);
}

/// C# properties.
#[test]
fn csharp_properties() {
    let source = r#"
public class User {
    public string Name { get; set; }
    public int Age { get; private set; }

    public User(string name, int age) {
        Name = name;
        Age = age;
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let user = chunks.iter().find(|c| c.ident == "User").unwrap();
    assert_eq!(user.kind, ChunkKind::Class);
}

/// C# interfaces with default implementation.
#[test]
fn csharp_interface_default_implementation() {
    let source = r#"
public interface ILogger {
    void Log(string message);

    void LogError(string message) {
        Log($"ERROR: {message}");
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let logger = chunks.iter().find(|c| c.ident == "ILogger").unwrap();
    assert_eq!(logger.kind, ChunkKind::Interface);
}

/// C# namespaces.
#[test]
fn csharp_namespaces() {
    let source = r#"
namespace MyApp {
    public class Service {
        public void Run() {}
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let ns = chunks.iter().find(|c| c.ident == "MyApp");
    assert!(ns.is_some(), "Should find namespace");

    let service = chunks.iter().find(|c| c.ident == "Service").unwrap();
    assert_eq!(service.kind, ChunkKind::Class);
}

// ============================================================
// PHASE 3b: Latest Language Features (C# 12+)
// ============================================================

/// Primary constructors (C# 12+).
#[test]
#[ignore = "C# primary constructors not supported in tree-sitter-c-sharp 0.23.1"]
fn csharp_primary_constructors() {
    let source = r#"
public class Person(string name, int age)
{
    public string Name => name;
    public int Age => age;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let c = chunks.iter().find(|c| c.ident == "Person");
    assert!(c.is_some(), "Should find class with primary constructor");
}

/// Collection expressions (C# 12+).
#[test]
#[ignore = "C# collection expressions not supported in tree-sitter-c-sharp 0.23.1"]
fn csharp_collection_expressions() {
    let source = r#"
public class Collections
{
    public void Demo()
    {
        int[] numbers = [1, 2, 3, 4, 5];
        List<string> names = ["Alice", "Bob"];
        var combined = [..numbers, 6, 7];
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "Demo");
    assert!(
        f.is_some(),
        "Should find method with collection expressions"
    );
}

/// Raw string literals (C# 11+).
#[test]
fn csharp_raw_string_literals() {
    let source = r####"
public class RawStrings
{
    public string GetJson()
    {
        return """
            {
                "name": "test",
                "value": 42
            }
            """;
    }
}
"####;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "GetJson");
    assert!(f.is_some(), "Should find method with raw string literal");
}

/// Required members (C# 11+).
#[test]
fn csharp_required_members() {
    let source = r#"
public class Person
{
    public required string Name { get; init; }
    public required int Age { get; init; }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let c = chunks.iter().find(|c| c.ident == "Person");
    assert!(c.is_some(), "Should find class with required members");
}

/// File-scoped namespaces (C# 10+).
#[test]
fn csharp_file_scoped_namespace() {
    let source = r#"
namespace MyApp;

public class Service
{
    public void Run() { }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let service = chunks.iter().find(|c| c.ident == "Service");
    assert!(
        service.is_some(),
        "Should find class in file-scoped namespace"
    );
}

/// Pattern matching enhancements.
#[test]
fn csharp_pattern_matching() {
    let source = r#"
public class Patterns
{
    public string Describe(object obj)
    {
        return obj switch
        {
            int i when i > 0 => "positive",
            int i when i < 0 => "negative",
            string { Length: > 10 } => "long string",
            null => "null",
            _ => "other"
        };
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "Describe");
    assert!(f.is_some(), "Should find method with pattern matching");
}

// ============================================================
// PHASE 4: Fallback Mechanism Tests
// ============================================================

/// Parse with quality: clean code should be Complete.
#[test]
fn parse_with_quality_clean() {
    use crate::ingest::code::CodeParser;

    let source = r#"
public class Valid {
    public void Method() {}
}
"#;
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(
        result.quality.is_complete(),
        "Clean code should have Complete quality"
    );
}

/// Parse with quality: syntax errors should recommend fallback.
#[test]
fn parse_with_quality_syntax_error() {
    use crate::ingest::code::CodeParser;

    let source = r#"
public class Broken {
    public void Method( {
    }
}
"#;
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(
        result.quality.fallback_recommended(),
        "Broken code should recommend fallback"
    );
}

// ============================================================
// PHASE 5: Edge Cases
// ============================================================

/// Empty file.
#[test]
fn empty_file() {
    let source = "";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.is_empty(), "Empty file should produce no chunks");
}

/// Comment-only file.
#[test]
fn comment_only_file() {
    let source = r#"
// Single line comment
/* Block comment */
/// XML doc comment
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(
        chunks.is_empty(),
        "Comment-only file should produce no code chunks"
    );
}

/// Partial valid code.
#[test]
fn partial_valid_code() {
    let source = r#"
public class Valid {
    public void Method() {}
}

public class Broken {
    public void Method( {
"#;
    let result = parser().parse_chunks(source, 1);
    assert!(result.is_ok(), "Should not crash on partial valid code");
}
