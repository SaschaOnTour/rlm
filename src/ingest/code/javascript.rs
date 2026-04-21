//! JavaScript/JSX parser for rlm.
//!
//! Supports ES6+ features including:
//! - Functions (regular, arrow, async, generator)
//! - Classes (methods, getters/setters, static members)
//! - ES Modules (import/export)
//! - `CommonJS` (require/module.exports)
//! - JSX Components

use tree_sitter::{Language, Query};

use crate::domain::chunk::{ChunkKind, RefKind};
use crate::infrastructure::parsing::tree_walker::{
    collect_prev_siblings, first_child_text_by_kind, SiblingCollectConfig,
};
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};

const CHUNK_QUERY_SRC: &str = include_str!("queries/javascript/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/javascript/ref.scm");

pub struct JavaScriptConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl JavaScriptConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_javascript::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "JavaScript",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for JavaScriptConfig {
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
        "javascript"
    }

    fn import_capture_name(&self) -> &'static str {
        "import_decl"
    }

    fn is_import_capture(&self, capture_name: &str) -> bool {
        capture_name == "import_decl" || capture_name == "require_decl"
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
            "class_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Class)),
            "method_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Method,
            )),
            n if n.ends_with("_def") => Some(ChunkCaptureResult::definition()),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "import_path" | "require_path" => Some(RefKind::Import),
            "jsx_component" => Some(RefKind::TypeUse),
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
            "import_path" | "require_path" => text.trim_matches('"').trim_matches('\'').to_string(),
            _ => text.to_string(),
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        extract_js_visibility(content)
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        extract_js_signature(content, kind)
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_js_parent(node, source)
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["comment"],
                skip_kinds: &[],
                prefixes: &["/**"],
                multi: false,
            },
        )
    }

    fn collect_attributes(&self, _node: tree_sitter::Node, _source: &[u8]) -> Option<String> {
        None // JS doesn't have attributes like decorators in this basic form
    }
}

/// Public type alias for the JavaScript parser.
pub type JavaScriptParser = BaseParser<JavaScriptConfig>;

impl Default for JavaScriptParser {
    fn default() -> Self {
        Self::new(JavaScriptConfig::new())
    }
}

impl JavaScriptParser {
    /// Create a new JavaScript parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(JavaScriptConfig::new())
    }
}

fn extract_js_visibility(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("export default") {
        Some("export default".into())
    } else if trimmed.starts_with("export") {
        Some("export".into())
    } else {
        None
    }
}

fn extract_js_signature(content: &str, kind: &ChunkKind) -> Option<String> {
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
        ChunkKind::Class => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        ChunkKind::Method => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        _ => None,
    }
}

fn find_js_parent(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "class_body" {
            let class_decl = parent.parent()?;
            if class_decl.kind() == "class_declaration" || class_decl.kind() == "class" {
                return first_child_text_by_kind(class_decl, source, &["identifier"]);
            }
        }
        current = parent.parent();
    }
    None
}

#[cfg(test)]
#[path = "javascript_advanced_tests.rs"]
mod advanced_tests;
#[cfg(test)]
#[path = "javascript_tests.rs"]
mod tests;
