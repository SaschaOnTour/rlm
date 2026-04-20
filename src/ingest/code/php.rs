use tree_sitter::{Language, Query};

use crate::domain::chunk::{ChunkKind, RefKind};
use crate::infrastructure::parsing::tree_walker::{
    collect_prev_siblings, find_parent_by_kind, SiblingCollectConfig,
};
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::ingest::code::helpers::{extract_keyword_visibility, extract_type_signature_to_brace};

const CHUNK_QUERY_SRC: &str = include_str!("queries/php/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/php/ref.scm");

pub struct PhpConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl PhpConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_php::LANGUAGE_PHP.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "PHP",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for PhpConfig {
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
        "php"
    }

    fn import_capture_name(&self) -> &'static str {
        "use_decl"
    }

    fn needs_deduplication(&self) -> bool {
        true
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "fn_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Function,
            )),
            "class_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Class)),
            "iface_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Interface,
            )),
            "method_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Method,
            )),
            "trait_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Trait)),
            n if n.ends_with("_def") => Some(ChunkCaptureResult::definition()),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "use_path" | "use_simple" => Some(RefKind::Import),
            "type_ref" | "type_ref_qualified" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        extract_keyword_visibility(content, "public", &[])
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        match kind {
            ChunkKind::Function | ChunkKind::Method => content
                .find('{')
                .map(|pos| content[..pos].trim().to_string()),
            ChunkKind::Class | ChunkKind::Interface | ChunkKind::Trait => {
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
                "interface_declaration",
                "trait_declaration",
            ],
            "name",
        )
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["comment"],
                skip_kinds: &["attribute_list"],
                prefixes: &["/**"],
                multi: false,
            },
        )
    }

    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["attribute_list", "attribute_group"],
                skip_kinds: &["comment"],
                prefixes: &[],
                multi: true,
            },
        )
    }
}

/// Public type alias for the PHP parser.
pub type PhpParser = BaseParser<PhpConfig>;

impl Default for PhpParser {
    fn default() -> Self {
        Self::new(PhpConfig::new())
    }
}

impl PhpParser {
    /// Create a new PHP parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(PhpConfig::new())
    }
}

#[cfg(test)]
#[path = "php_advanced_tests.rs"]
mod advanced_tests;
#[cfg(test)]
#[path = "php_tests.rs"]
mod tests;
