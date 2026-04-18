//! Rust parser — `LanguageConfig` wiring on top of the shared `BaseParser`.
//!
//! Slice 4.3 (parser pilot) made this the reference parser for the new
//! layout: tree-walker helpers pull directly from
//! `infrastructure::parsing::tree_walker` rather than the legacy
//! re-export chain through `ingest::code::base`, and tests live in the
//! companion file `rust_tests.rs`.

use tree_sitter::{Language, Query, Tree};

use crate::infrastructure::parsing::tree_walker::{
    collect_prev_siblings, collect_prev_siblings_filtered_skip, find_parent_by_kind,
    SiblingCollectConfig,
};
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::ingest::code::helpers::extract_type_signature;
use crate::models::chunk::{Chunk, ChunkKind, RefKind};

use super::rust_impl_methods::{extract_impl_methods, find_node_at_byte_range};

const CHUNK_QUERY_SRC: &str = include_str!("queries/rust/chunk.scm");
const REF_QUERY_SRC: &str = include_str!("queries/rust/ref.scm");

pub struct RustConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl RustConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_rust::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "Rust",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for RustConfig {
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
        "rust"
    }

    fn import_capture_name(&self) -> &'static str {
        "use_decl"
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "fn_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Function,
            )),
            "struct_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Struct,
            )),
            "enum_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Enum)),
            "trait_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Trait)),
            "impl_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Impl)),
            "const_name" | "static_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Constant,
            )),
            "mod_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Module,
            )),
            "macro_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Other("macro".into()),
            )),
            "type_alias_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Other("type_alias".into()),
            )),
            "fn_def" | "struct_def" | "enum_def" | "trait_def" | "impl_def" | "const_def"
            | "static_def" | "mod_def" | "macro_def" | "type_alias_def" => {
                Some(ChunkCaptureResult::definition())
            }
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "scoped_call" | "method_call" => Some(RefKind::Call),
            "use_name" | "use_path" | "use_as_path" | "use_list_item" | "use_list_scoped"
            | "use_group_path" | "use_simple" => Some(RefKind::Import),
            "type_ref" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        extract_rust_visibility(content)
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        match kind {
            ChunkKind::Function => extract_fn_signature(content),
            ChunkKind::Struct | ChunkKind::Enum | ChunkKind::Trait => {
                extract_type_signature(content)
            }
            _ => None,
        }
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_parent_by_kind(node, source, &["impl_item"], "type_identifier")
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(node, source, &SiblingCollectConfig::rust_doc_comments())
    }

    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings_filtered_skip(node, source, &SiblingCollectConfig::rust_attributes())
    }

    fn should_skip_function(&self, kind: &ChunkKind, parent: &Option<String>) -> bool {
        // Skip functions inside impl blocks - captured as methods by post_process_chunks.
        *kind == ChunkKind::Function && parent.is_some()
    }

    fn post_process_chunks(
        &self,
        chunks: &mut Vec<Chunk>,
        tree: &Tree,
        source: &[u8],
        file_id: i64,
    ) {
        let impl_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Impl)
            .map(|c| (c.ident.clone(), c.start_byte, c.end_byte))
            .collect();

        for (impl_name, start_byte, end_byte) in &impl_chunks {
            let root = tree.root_node();
            if let Some(impl_node) =
                find_node_at_byte_range(root, *start_byte as usize, *end_byte as usize)
            {
                let methods = extract_impl_methods(impl_node, source, file_id, impl_name);
                chunks.extend(methods);
            }
        }
    }
}

pub type RustParser = BaseParser<RustConfig>;

impl Default for RustParser {
    fn default() -> Self {
        Self::new(RustConfig::new())
    }
}

impl RustParser {
    #[must_use]
    pub fn create() -> Self {
        Self::new(RustConfig::new())
    }
}

// =============================================================================
// Language-specific helpers
// =============================================================================

pub(crate) fn extract_rust_visibility(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("pub(crate)") {
        Some("pub(crate)".into())
    } else if trimmed.starts_with("pub(super)") {
        Some("pub(super)".into())
    } else if trimmed.starts_with("pub") {
        Some("pub".into())
    } else {
        Some("private".into())
    }
}

pub(crate) fn extract_fn_signature(content: &str) -> Option<String> {
    if let Some(brace_pos) = content.find('{') {
        let sig = content[..brace_pos].trim();
        Some(sig.to_string())
    } else {
        content
            .find(';')
            .map(|semi_pos| content[..semi_pos].trim().to_string())
    }
}

#[cfg(test)]
#[path = "rust_tests.rs"]
mod tests;
