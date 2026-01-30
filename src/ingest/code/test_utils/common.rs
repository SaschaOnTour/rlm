//! Common test utilities and macros for parser testing.
//!
//! This module provides shared test macros that can be used across all language parsers
//! to ensure consistent testing of critical functionality like byte offsets, unicode
//! handling, line ending normalization, and reference positioning.

/// Macro to generate standard parser reliability tests.
///
/// These tests are critical for ensuring surgical editing operations work correctly.
///
/// # Usage
/// ```ignore
/// parser_reliability_tests!(RustParser, "rust");
/// ```
#[macro_export]
macro_rules! parser_reliability_tests {
    ($parser_type:ty, $lang_name:expr, $sample_code:expr) => {
        mod reliability_tests {
            use super::*;

            fn parser() -> $parser_type {
                <$parser_type>::new()
            }

            /// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
            #[test]
            fn byte_offset_round_trip() {
                let source = $sample_code;
                let chunks = parser()
                    .parse_chunks(source, 1)
                    .expect("parsing should succeed");

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
                         Expected bytes {}..{} to equal content",
                        chunk.ident, chunk.kind, chunk.start_byte, chunk.end_byte
                    );
                }
            }

            /// CRLF line endings must not break line number calculation.
            #[test]
            fn handles_crlf_line_endings() {
                let source = $sample_code.replace('\n', "\r\n");
                let result = parser().parse_chunks(&source, 1);
                assert!(result.is_ok(), "Should handle CRLF line endings");
            }

            /// Empty file should not crash.
            #[test]
            fn handles_empty_file() {
                let chunks = parser().parse_chunks("", 1).unwrap();
                assert!(chunks.is_empty(), "Empty file should produce no chunks");
            }

            /// Partial valid code should not crash.
            #[test]
            fn handles_partial_code() {
                // Truncate the sample at a semi-random point
                let source = $sample_code;
                let truncated = &source[..source.len().min(50)];
                let result = parser().parse_chunks(truncated, 1);
                assert!(result.is_ok(), "Should not crash on partial code");
            }

            /// Parse with quality should work.
            #[test]
            fn parse_with_quality_works() {
                use $crate::ingest::code::CodeParser;

                let source = $sample_code;
                let result = parser().parse_with_quality(source, 1);
                assert!(result.is_ok(), "parse_with_quality should succeed");
            }

            /// Validate syntax should work.
            #[test]
            fn validate_syntax_works() {
                use $crate::ingest::code::CodeParser;

                let source = $sample_code;
                // Don't assert the result - just ensure it doesn't panic
                let _ = parser().validate_syntax(source);
            }
        }
    };
}

/// Macro to test that a specific code pattern is correctly parsed.
///
/// # Usage
/// ```ignore
/// test_parses_chunk!(RustParser, "fn main() {}", "main", ChunkKind::Function);
/// ```
#[macro_export]
macro_rules! test_parses_chunk {
    ($parser_type:ty, $source:expr, $expected_name:expr, $expected_kind:expr) => {{
        let parser = <$parser_type>::new();
        let chunks = parser
            .parse_chunks($source, 1)
            .expect("parsing should succeed");
        let found = chunks
            .iter()
            .find(|c| c.ident == $expected_name && c.kind == $expected_kind);
        assert!(
            found.is_some(),
            "Expected to find chunk '{}' with kind {:?}, but found: {:?}",
            $expected_name,
            $expected_kind,
            chunks
                .iter()
                .map(|c| (&c.ident, &c.kind))
                .collect::<Vec<_>>()
        );
        found.unwrap().clone()
    }};
}

/// Macro to test that imports are correctly extracted.
///
/// # Usage
/// ```ignore
/// test_extracts_imports!(RustParser, "use std::collections::HashMap;", 1);
/// ```
#[macro_export]
macro_rules! test_extracts_imports {
    ($parser_type:ty, $source:expr, $min_count:expr) => {{
        use $crate::models::chunk::RefKind;

        let parser = <$parser_type>::new();
        let chunks = parser
            .parse_chunks($source, 1)
            .expect("parsing should succeed");

        // Check for _imports chunk
        let imports_chunk = chunks.iter().find(|c| c.ident == "_imports");
        assert!(
            imports_chunk.is_some(),
            "Should have an _imports chunk for source:\n{}",
            $source
        );

        // Check for import refs
        let refs = parser
            .extract_refs($source, &chunks)
            .expect("ref extraction should succeed");
        let import_refs: Vec<_> = refs
            .iter()
            .filter(|r| r.ref_kind == RefKind::Import)
            .collect();

        assert!(
            import_refs.len() >= $min_count,
            "Should capture at least {} import refs, got {}",
            $min_count,
            import_refs.len()
        );
    }};
}

/// Macro to test visibility extraction.
///
/// # Usage
/// ```ignore
/// test_visibility!(RustParser, "pub fn foo() {}", "foo", "pub");
/// ```
#[macro_export]
macro_rules! test_visibility {
    ($parser_type:ty, $source:expr, $name:expr, $expected_visibility:expr) => {{
        let parser = <$parser_type>::new();
        let chunks = parser
            .parse_chunks($source, 1)
            .expect("parsing should succeed");
        let chunk = chunks
            .iter()
            .find(|c| c.ident == $name)
            .expect(&format!("Should find chunk '{}'", $name));
        assert_eq!(
            chunk.visibility.as_deref(),
            Some($expected_visibility),
            "Visibility mismatch for '{}'",
            $name
        );
    }};
}

/// Macro to test signature extraction.
///
/// # Usage
/// ```ignore
/// test_signature_contains!(RustParser, "pub fn foo(x: i32) { }", "foo", "pub fn foo");
/// ```
#[macro_export]
macro_rules! test_signature_contains {
    ($parser_type:ty, $source:expr, $name:expr, $expected_substring:expr) => {{
        let parser = <$parser_type>::new();
        let chunks = parser
            .parse_chunks($source, 1)
            .expect("parsing should succeed");
        let chunk = chunks
            .iter()
            .find(|c| c.ident == $name)
            .expect(&format!("Should find chunk '{}'", $name));
        assert!(
            chunk.signature.is_some(),
            "'{}' should have a signature",
            $name
        );
        assert!(
            chunk
                .signature
                .as_ref()
                .unwrap()
                .contains($expected_substring),
            "Signature of '{}' should contain '{}', got: {:?}",
            $name,
            $expected_substring,
            chunk.signature
        );
    }};
}

/// Macro to test parent relationship.
///
/// # Usage
/// ```ignore
/// test_parent!(RustParser, "impl Foo { fn bar() {} }", "bar", "Foo");
/// ```
#[macro_export]
macro_rules! test_parent {
    ($parser_type:ty, $source:expr, $name:expr, $expected_parent:expr) => {{
        let parser = <$parser_type>::new();
        let chunks = parser
            .parse_chunks($source, 1)
            .expect("parsing should succeed");
        let chunk = chunks
            .iter()
            .find(|c| c.ident == $name)
            .expect(&format!("Should find chunk '{}'", $name));
        assert_eq!(
            chunk.parent.as_deref(),
            Some($expected_parent),
            "Parent mismatch for '{}'",
            $name
        );
    }};
}

/// Macro to test that references are within their containing chunks.
///
/// # Usage
/// ```ignore
/// test_refs_within_chunks!(RustParser, "fn caller() { helper(); }");
/// ```
#[macro_export]
macro_rules! test_refs_within_chunks {
    ($parser_type:ty, $source:expr) => {{
        let parser = <$parser_type>::new();
        let (chunks, refs) = parser
            .parse_chunks_and_refs($source, 1)
            .expect("parsing should succeed");

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
    }};
}

/// Sample code for Rust parser tests.
pub const RUST_SAMPLE: &str = r#"
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

/// Sample code for Go parser tests.
pub const GO_SAMPLE: &str = r#"
package main

func Hello(name string) string {
    return "Hello, " + name
}

type Config struct {
    Name string
    Port int
}
"#;

/// Sample code for Java parser tests.
pub const JAVA_SAMPLE: &str = r#"
public class Main {
    public static void main(String[] args) {
        System.out.println("Hello");
    }

    private int helper(int x) {
        return x * 2;
    }
}
"#;

/// Sample code for Python parser tests.
pub const PYTHON_SAMPLE: &str = r#"
def hello(name):
    return f"Hello, {name}"

class Config:
    def __init__(self, name):
        self.name = name
"#;

/// Sample code for C# parser tests.
pub const CSHARP_SAMPLE: &str = r#"
public class Main {
    public static void Main(string[] args) {
        Console.WriteLine("Hello");
    }

    private int Helper(int x) {
        return x * 2;
    }
}
"#;

/// Sample code for PHP parser tests.
pub const PHP_SAMPLE: &str = r#"<?php
class Main {
    public function process(): string {
        return "done";
    }

    private function helper(int $x): int {
        return $x * 2;
    }
}
"#;
