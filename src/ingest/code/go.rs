use tree_sitter::{Language, Query};

use crate::ingest::code::base::{
    build_language_config, collect_prev_siblings, extract_type_signature_to_brace, BaseParser,
    ChunkCaptureResult, LanguageConfig, SiblingCollectConfig,
};
use crate::models::chunk::{ChunkKind, RefKind};

const CHUNK_QUERY_SRC: &str = include_str!("queries/go/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/go/ref.scm");

pub struct GoConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl GoConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_go::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "Go",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for GoConfig {
    fn language(&self) -> &Language {
        &self.language
    }

    fn chunk_query(&self) -> &Query {
        &self.chunk_query
    }

    fn ref_query(&self) -> &Query {
        &self.ref_query
    }

    fn language_name(&self) -> &'static str {
        "go"
    }

    fn import_capture_name(&self) -> &'static str {
        "import_decl"
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "fn_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Function,
            )),
            "method_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Method,
            )),
            "type_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Struct,
            )),
            "fn_def" | "method_def" | "type_def" => Some(ChunkCaptureResult::definition()),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "import_path" | "import_alias" => Some(RefKind::Import),
            "type_ref" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        // Go convention: uppercase first letter = exported (pub), lowercase = private
        let first_char = content
            .split_whitespace()
            .find(|w| !w.starts_with("func") && !w.starts_with("type"))
            .and_then(|w| w.chars().next());
        match first_char {
            Some(c) if c.is_uppercase() => Some("pub".into()),
            _ => Some("private".into()),
        }
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        match kind {
            ChunkKind::Function | ChunkKind::Method => content
                .find('{')
                .map(|pos| content[..pos].trim().to_string()),
            ChunkKind::Struct => extract_type_signature_to_brace(content),
            _ => None,
        }
    }

    fn find_parent(&self, _node: tree_sitter::Node, _source: &[u8]) -> Option<String> {
        None // Go doesn't have nested types like Rust impl blocks
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["comment"],
                skip_kinds: &[],
                prefixes: &[],
                multi: true,
            },
        )
    }

    fn collect_attributes(&self, _node: tree_sitter::Node, _source: &[u8]) -> Option<String> {
        None // Go doesn't have attributes/annotations
    }
}

/// Public type alias for the Go parser.
pub type GoParser = BaseParser<GoConfig>;

impl Default for GoParser {
    fn default() -> Self {
        Self::new(GoConfig::new())
    }
}

impl GoParser {
    /// Create a new Go parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(GoConfig::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::code::CodeParser;
    use crate::models::chunk::{ChunkKind, RefKind};

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
}
