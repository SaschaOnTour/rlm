use tree_sitter::{Language, Query};

use crate::domain::chunk::{ChunkKind, RefKind};
use crate::infrastructure::parsing::tree_walker::{
    collect_prev_siblings, collect_prev_siblings_filtered_skip, find_parent_by_kind,
    SiblingCollectConfig,
};
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::ingest::code::helpers::{extract_keyword_visibility, extract_type_signature_to_brace};

const CHUNK_QUERY_SRC: &str = include_str!("queries/csharp/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/csharp/ref.scm");

pub struct CSharpConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl CSharpConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_c_sharp::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "C#",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for CSharpConfig {
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
        "csharp"
    }

    fn import_capture_name(&self) -> &'static str {
        "using_decl"
    }

    fn needs_deduplication(&self) -> bool {
        true
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "class_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Class)),
            "iface_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Interface,
            )),
            "enum_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Enum)),
            "struct_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Struct,
            )),
            "method_name" | "ctor_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Method,
            )),
            "ns_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Module,
            )),
            n if n.ends_with("_def") => Some(ChunkCaptureResult::definition()),
            _ => None,
        }
    }

    // qual:allow(dry) reason: "language-specific ref kind mapping inherently similar across parsers"
    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "using_path" | "using_simple" => Some(RefKind::Import),
            "type_ref" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        extract_keyword_visibility(content, "private", &[("internal", "internal")])
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        match kind {
            ChunkKind::Method => content
                .find('{')
                .map(|pos| content[..pos].trim().to_string()),
            ChunkKind::Class | ChunkKind::Interface | ChunkKind::Enum | ChunkKind::Struct => {
                extract_type_signature_to_brace(content)
            }
            _ => None,
        }
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_parent_by_kind(
            node,
            source,
            &[
                "class_declaration",
                "struct_declaration",
                "interface_declaration",
            ],
            "identifier",
        )
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["comment"],
                skip_kinds: &["attribute_list"],
                prefixes: &["///"],
                multi: true,
            },
        )
    }

    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings_filtered_skip(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["attribute_list"],
                skip_kinds: &["comment"],
                prefixes: &["///"],
                multi: true,
            },
        )
    }
}

/// Public type alias for the C# parser.
pub type CSharpParser = BaseParser<CSharpConfig>;

impl Default for CSharpParser {
    fn default() -> Self {
        Self::new(CSharpConfig::new())
    }
}

impl CSharpParser {
    /// Create a new C# parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(CSharpConfig::new())
    }
}

#[cfg(test)]
#[path = "csharp_advanced_tests.rs"]
mod advanced_tests;
#[cfg(test)]
#[path = "csharp_tests.rs"]
mod tests;
