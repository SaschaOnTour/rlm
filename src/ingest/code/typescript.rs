//! TypeScript/TSX parser for rlm.
//!
//! Supports TypeScript-specific features including:
//! - Interfaces and Type Aliases
//! - Generics and Type Parameters
//! - Decorators
//! - Enums
//! - Namespaces/Modules
//! - All JavaScript features via shared base

use tree_sitter::{Language, Query};

use crate::infrastructure::parsing::tree_walker::{
    collect_prev_siblings, first_child_text_by_kind, SiblingCollectConfig,
};
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::models::chunk::{ChunkKind, RefKind};

const CHUNK_QUERY_SRC: &str = include_str!("queries/typescript/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/typescript/ref.scm");

// TSX-specific query additions (JSX elements)
const TSX_REF_QUERY_ADDITION: &str = r"
    ; JSX/TSX elements
    (jsx_element
        open_tag: (jsx_opening_element
            name: (identifier) @jsx_component))
    (jsx_self_closing_element
        name: (identifier) @jsx_component)
";

pub struct TypeScriptConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl TypeScriptConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "TypeScript",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }

    fn new_tsx() -> Self {
        // TSX includes JSX elements in refs
        let tsx_ref_query = format!("{REF_QUERY_SRC}\n{TSX_REF_QUERY_ADDITION}");
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            CHUNK_QUERY_SRC,
            &tsx_ref_query,
            "TSX",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for TypeScriptConfig {
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
        "typescript"
    }

    fn import_capture_name(&self) -> &'static str {
        "import_decl"
    }

    fn needs_deduplication(&self) -> bool {
        true
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "fn_name" | "gen_fn_name" | "arrow_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Function,
            )),
            "class_name" | "abs_class_name" => {
                Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Class))
            }
            "method_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Method,
            )),
            "iface_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Interface,
            )),
            "type_alias_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Other("type_alias".into()),
            )),
            "enum_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Enum)),
            "namespace_name" | "internal_namespace_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Module,
            )),
            n if n.ends_with("_def") => Some(ChunkCaptureResult::definition()),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "import_path" => Some(RefKind::Import),
            "type_ref" | "generic_type_ref" => Some(RefKind::TypeUse),
            "jsx_component" => Some(RefKind::TypeUse),
            "decorator_name" => Some(RefKind::Call),
            _ => None,
        }
    }

    fn filter_ref_capture(&self, capture_name: &str, text: &str) -> bool {
        if capture_name == "jsx_component" {
            // Only PascalCase names are components
            text.chars().next().is_some_and(char::is_uppercase)
        } else {
            true
        }
    }

    fn transform_ref_text(&self, capture_name: &str, text: &str) -> String {
        match capture_name {
            // Clean up string quotes from import paths
            "import_path" => text.trim_matches('"').trim_matches('\'').to_string(),
            _ => text.to_string(),
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        extract_ts_visibility(content)
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        extract_ts_signature(content, kind)
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_ts_parent(node, source)
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["comment"],
                skip_kinds: &["decorator"],
                prefixes: &["/**", "//"],
                multi: false,
            },
        )
    }

    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["decorator"],
                skip_kinds: &["comment"],
                prefixes: &[],
                multi: true,
            },
        )
    }
}

/// Public type alias for the TypeScript parser.
pub type TypeScriptParser = BaseParser<TypeScriptConfig>;

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new(TypeScriptConfig::new())
    }
}

impl TypeScriptParser {
    /// Create a new TypeScript parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(TypeScriptConfig::new())
    }

    /// Create a TSX parser for .tsx files.
    #[must_use]
    pub fn create_tsx() -> Self {
        Self::new(TypeScriptConfig::new_tsx())
    }
}

/// Visibility keywords to check, ordered so longer prefixes come first.
const TS_VISIBILITY_KEYWORDS: &[&str] =
    &["export default", "export", "public", "private", "protected"];

fn extract_ts_visibility(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    TS_VISIBILITY_KEYWORDS
        .iter()
        .find(|kw| trimmed.starts_with(**kw))
        .map(|kw| (*kw).to_string())
}

fn extract_ts_signature(content: &str, kind: &ChunkKind) -> Option<String> {
    match kind {
        ChunkKind::Function => {
            // Find opening brace or arrow
            if let Some(brace_pos) = content.find('{') {
                Some(content[..brace_pos].trim().to_string())
            } else if let Some(arrow_pos) = content.find("=>") {
                Some(content[..arrow_pos + 2].trim().to_string())
            } else {
                content.lines().next().map(|s| s.trim().to_string())
            }
        }
        ChunkKind::Class | ChunkKind::Interface => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        ChunkKind::Method => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        ChunkKind::Enum => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        ChunkKind::Other(s) if s == "type_alias" => {
            // Type alias: type Foo = ...
            content
                .find('=')
                .map(|pos| content[..pos].trim().to_string())
        }
        _ => content.lines().next().map(|s| s.trim().to_string()),
    }
}

/// Describes a parent context found by walking up the tree.
enum TsParentContext<'a> {
    /// Found a class_body whose parent is a class declaration.
    ClassDecl(tree_sitter::Node<'a>),
    /// Found an interface declaration.
    InterfaceDecl(tree_sitter::Node<'a>),
}

/// Walk up the tree to find the enclosing class or interface (operation: logic only).
///
/// Returns the relevant parent node without extracting its name.
fn find_ts_parent_context(node: tree_sitter::Node) -> Option<TsParentContext> {
    let mut current = node.parent();
    while let Some(parent) = current {
        let kind = parent.kind();
        if kind == "class_body" {
            let class_decl = parent.parent()?;
            if class_decl.kind() == "class_declaration" || class_decl.kind() == "class" {
                return Some(TsParentContext::ClassDecl(class_decl));
            }
        } else if kind == "interface_declaration" {
            return Some(TsParentContext::InterfaceDecl(parent));
        }
        current = parent.parent();
    }
    None
}

/// Extract the parent name from a TypeScript parent context (integration: calls only).
fn find_ts_parent(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    match find_ts_parent_context(node)? {
        TsParentContext::ClassDecl(decl) => {
            first_child_text_by_kind(decl, source, &["type_identifier", "identifier"])
        }
        TsParentContext::InterfaceDecl(decl) => {
            first_child_text_by_kind(decl, source, &["type_identifier"])
        }
    }
}

#[cfg(test)]
#[path = "typescript_tests.rs"]
mod tests;
