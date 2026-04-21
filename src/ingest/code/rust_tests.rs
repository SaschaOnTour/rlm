//! Parser tests for `rust.rs` (PHASE 1–2).
//!
//! Moved out of `rust.rs` in slice 4.3 (parser pilot) so the production
//! file stays focused on `LanguageConfig` wiring; wired back in via
//! `#[cfg(test)] #[path = "rust_tests.rs"] mod tests;`.
//!
//! Advanced feature / edge-case tests (PHASE 3 onward) live in the sibling
//! `rust_advanced_tests.rs` to keep each companion focused on one cluster
//! of behaviors (SRP_MODULE).

use super::RustParser;
use crate::domain::chunk::{ChunkKind, RefKind};
use crate::ingest::code::CodeParser;

fn parser() -> RustParser {
    RustParser::create()
}

#[test]
fn parse_function() {
    let source = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {}", name)
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty());
    let f = chunks.iter().find(|c| c.ident == "hello").unwrap();
    assert_eq!(f.kind, ChunkKind::Function);
    assert_eq!(f.visibility.as_deref(), Some("pub"));
    assert!(f.signature.as_ref().unwrap().contains("pub fn hello"));
}

#[test]
fn parse_struct() {
    let source = r#"
pub struct Config {
    pub root: PathBuf,
    pub db_path: PathBuf,
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let s = chunks.iter().find(|c| c.ident == "Config").unwrap();
    assert_eq!(s.kind, ChunkKind::Struct);
}

#[test]
fn parse_impl_with_methods() {
    let source = r#"
impl Config {
    pub fn new() -> Self {
        Self { root: PathBuf::new(), db_path: PathBuf::new() }
    }

    fn private_method(&self) -> bool {
        true
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Config" && c.kind == ChunkKind::Impl));
    let new_method = chunks
        .iter()
        .find(|c| c.ident == "new" && c.kind == ChunkKind::Method);
    assert!(new_method.is_some());
    let m = new_method.unwrap();
    assert_eq!(m.parent.as_deref(), Some("Config"));
    assert_eq!(m.visibility.as_deref(), Some("pub"));
}

#[test]
fn parse_enum() {
    let source = r#"
pub enum Color {
    Red,
    Green,
    Blue,
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let e = chunks.iter().find(|c| c.ident == "Color").unwrap();
    assert_eq!(e.kind, ChunkKind::Enum);
}

#[test]
fn parse_trait() {
    let source = r#"
pub trait Parser {
    fn parse(&self, input: &str) -> Result<()>;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let t = chunks.iter().find(|c| c.ident == "Parser").unwrap();
    assert_eq!(t.kind, ChunkKind::Trait);
}

#[test]
fn extract_refs_finds_calls() {
    let source = r#"
fn main() {
    let x = hello("world");
    println!("{}", x);
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let refs = parser().extract_refs(source, &chunks).unwrap();
    assert!(refs
        .iter()
        .any(|r| r.target_ident == "hello" && r.ref_kind == RefKind::Call));
}

#[test]
fn validate_syntax_valid() {
    assert!(parser().validate_syntax("fn main() {}"));
}

#[test]
fn validate_syntax_invalid() {
    assert!(!parser().validate_syntax("fn main() {"));
}

// Regression tests for parser fixes

#[test]
fn extract_imports_all_patterns() {
    let source = r#"
use std::collections::HashMap;
use crate::error::Result;
use super::helper;
use foo::{bar, baz};
use some::path as alias;
use simple;
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

    // Should capture multiple import targets
    assert!(
        import_refs.len() >= 5,
        "Should capture at least 5 import refs, got {}",
        import_refs.len()
    );

    // Check specific imports are captured
    let import_names: Vec<&str> = import_refs
        .iter()
        .map(|r| r.target_ident.as_str())
        .collect();
    assert!(
        import_names
            .iter()
            .any(|n| n.contains("HashMap") || n.contains("collections")),
        "Should capture HashMap or collections"
    );
}

#[test]
fn no_duplicate_methods_in_impl() {
    let source = r#"
impl Config {
    pub fn new() -> Self { Self {} }
    fn helper(&self) -> bool { true }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    // Count chunks named "new"
    let new_chunks: Vec<_> = chunks.iter().filter(|c| c.ident == "new").collect();
    assert_eq!(
        new_chunks.len(),
        1,
        "Should have exactly 1 'new' chunk, not duplicates, got {}",
        new_chunks.len()
    );
    assert_eq!(new_chunks[0].kind, ChunkKind::Method);

    // Count chunks named "helper"
    let helper_chunks: Vec<_> = chunks.iter().filter(|c| c.ident == "helper").collect();
    assert_eq!(
        helper_chunks.len(),
        1,
        "Should have exactly 1 'helper' chunk, got {}",
        helper_chunks.len()
    );
}

#[test]
fn struct_enum_trait_have_signatures() {
    let source = r#"
pub struct Config {
    pub name: String,
}

pub enum Status {
    Active,
    Inactive,
}

pub trait Handler {
    fn handle(&self);
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
            .contains("pub struct Config"),
        "Config signature should contain 'pub struct Config'"
    );

    let status = chunks.iter().find(|c| c.ident == "Status").unwrap();
    assert!(status.signature.is_some(), "Status should have a signature");
    assert!(
        status
            .signature
            .as_ref()
            .unwrap()
            .contains("pub enum Status"),
        "Status signature should contain 'pub enum Status'"
    );

    let handler = chunks.iter().find(|c| c.ident == "Handler").unwrap();
    assert!(
        handler.signature.is_some(),
        "Handler should have a signature"
    );
    assert!(
        handler
            .signature
            .as_ref()
            .unwrap()
            .contains("pub trait Handler"),
        "Handler signature should contain 'pub trait Handler'"
    );
}

// ============================================================
// PHASE 2: Critical Reliability Tests
// ============================================================

/// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
/// This is essential for surgical editing operations.
#[test]
fn byte_offset_round_trip() {
    let source = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {}", name)
}

pub struct Config {
    pub root: PathBuf,
}

impl Config {
    pub fn new() -> Self {
        Self { root: PathBuf::new() }
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty(), "Should have extracted chunks");

    for chunk in &chunks {
        // Skip imports chunk as it may join multiple nodes
        if chunk.ident == "_imports" {
            continue;
        }
        let reconstructed = &source[chunk.start_byte as usize..chunk.end_byte as usize];
        assert_eq!(
            reconstructed, chunk.content,
            "Byte offset reconstruction failed for chunk '{}' (kind: {:?})\n\
             Expected bytes {}..{} to equal content:\n---\n{}\n---\n\
             But got:\n---\n{}\n---",
            chunk.ident, chunk.kind, chunk.start_byte, chunk.end_byte, chunk.content, reconstructed
        );
    }
}

/// Unicode identifiers must be handled correctly.
#[test]
fn unicode_identifiers() {
    let source = r#"
fn größe() -> usize {
    42
}

fn 日本語() {
    println!("こんにちは");
}

struct Größe {
    wert: u32,
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let groesse = chunks.iter().find(|c| c.ident == "größe");
    assert!(groesse.is_some(), "Should find function with German umlaut");

    let japanese = chunks.iter().find(|c| c.ident == "日本語");
    assert!(
        japanese.is_some(),
        "Should find function with Japanese name"
    );

    let struct_groesse = chunks.iter().find(|c| c.ident == "Größe");
    assert!(
        struct_groesse.is_some(),
        "Should find struct with German umlaut"
    );
}

/// Unicode byte offsets must be correct (not just char indices).
#[test]
fn unicode_byte_offset_accuracy() {
    let source = "fn größe() { let x = 1; }\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let f = chunks.iter().find(|c| c.ident == "größe").unwrap();
    let reconstructed = &source[f.start_byte as usize..f.end_byte as usize];
    assert_eq!(
        reconstructed, f.content,
        "Unicode byte offsets must be exact"
    );
}

/// CRLF line endings must not break line number calculation.
#[test]
fn crlf_line_endings() {
    let source = "fn foo() {\r\n    bar()\r\n}\r\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let f = chunks.iter().find(|c| c.ident == "foo").unwrap();
    assert_eq!(f.start_line, 1, "Start line should be 1");
    assert_eq!(
        f.end_line, 3,
        "End line should be 3 (CRLF counted correctly)"
    );
}

/// Mixed line endings (Unix, Windows, old Mac).
#[test]
fn mixed_line_endings() {
    let source = "fn one() {}\nfn two() {}\r\nfn three() {}\rfn four() {}";
    let chunks = parser().parse_chunks(source, 1).unwrap();

    assert!(chunks.iter().any(|c| c.ident == "one"), "Should find 'one'");
    assert!(chunks.iter().any(|c| c.ident == "two"), "Should find 'two'");
    assert!(
        chunks.iter().any(|c| c.ident == "three"),
        "Should find 'three'"
    );
    // Note: \r alone may or may not be handled depending on tree-sitter
}

/// Reference positions must be within their containing chunk's line range.
#[test]
fn reference_positions_within_chunks() {
    let source = r#"
fn caller() {
    let x = helper();
    let y = other_fn();
}

fn helper() -> i32 { 42 }
fn other_fn() -> i32 { 0 }
"#;
    let (chunks, refs) = parser().parse_chunks_and_refs(source, 1).unwrap();

    // All refs with a non-zero chunk_id should be within that chunk's range
    for r in &refs {
        if r.chunk_id != 0 {
            let chunk = chunks.iter().find(|c| c.id == r.chunk_id);
            if let Some(c) = chunk {
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

// ─── enum variants (task #116) ─────────────────────────────────────────

#[test]
fn parse_enum_variants_unit() {
    let source = r#"
pub enum Color {
    Red,
    Green,
    Blue,
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let red = chunks
        .iter()
        .find(|c| c.ident == "Red")
        .expect("Red variant should be indexed as a chunk");
    assert_eq!(red.kind, ChunkKind::EnumVariant);
    assert_eq!(red.parent.as_deref(), Some("Color"));

    for name in ["Red", "Green", "Blue"] {
        assert!(
            chunks
                .iter()
                .any(|c| c.ident == name && c.kind == ChunkKind::EnumVariant),
            "missing variant chunk for {name}"
        );
    }
}

#[test]
fn parse_enum_variants_tuple_and_struct() {
    let source = r#"
pub enum Value {
    Int(i64),
    Pair(i64, i64),
    Named { name: String, age: u32 },
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let int = chunks.iter().find(|c| c.ident == "Int").unwrap();
    assert_eq!(int.kind, ChunkKind::EnumVariant);
    assert_eq!(int.parent.as_deref(), Some("Value"));
    assert!(int.content.contains("Int(i64)"), "got: {}", int.content);

    let pair = chunks.iter().find(|c| c.ident == "Pair").unwrap();
    assert!(
        pair.content.contains("Pair(i64, i64)"),
        "got: {}",
        pair.content
    );

    let named = chunks.iter().find(|c| c.ident == "Named").unwrap();
    assert!(
        named.content.contains("name: String"),
        "got: {}",
        named.content
    );
}

#[test]
fn parse_enum_variants_preserve_doc_and_attrs() {
    let source = r#"
pub enum Level {
    /// Low level.
    Low,
    #[deprecated]
    High,
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let low = chunks.iter().find(|c| c.ident == "Low").unwrap();
    assert!(
        low.doc_comment
            .as_deref()
            .unwrap_or("")
            .contains("Low level"),
        "doc comment not attached, got: {:?}",
        low.doc_comment
    );
    let high = chunks.iter().find(|c| c.ident == "High").unwrap();
    assert!(
        high.attributes
            .as_deref()
            .unwrap_or("")
            .contains("deprecated"),
        "attribute not attached, got: {:?}",
        high.attributes
    );
}
