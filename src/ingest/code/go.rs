use tree_sitter::{Language, Query};

use crate::infrastructure::parsing::tree_walker::{collect_prev_siblings, SiblingCollectConfig};
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::ingest::code::helpers::extract_type_signature_to_brace;
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
#[path = "go_tests.rs"]
mod tests;
