use tree_sitter::{Language, Query};

use crate::infrastructure::parsing::tree_walker::find_parent_by_kind;
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::ingest::code::helpers::extract_type_signature_to_colon;
use crate::models::chunk::{ChunkKind, RefKind};

const CHUNK_QUERY_SRC: &str = include_str!("queries/python/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/python/ref.scm");

pub struct PythonConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl PythonConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_python::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "Python",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for PythonConfig {
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
        "python"
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
            "class_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Class)),
            "fn_def" | "class_def" => Some(ChunkCaptureResult::definition()),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "import_name" | "import_from_module" | "import_from_name" | "import_alias" => {
                Some(RefKind::Import)
            }
            "type_ref" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn extract_visibility(&self, _content: &str) -> Option<String> {
        // Python visibility is based on name, not content.
        // We handle it in adjust_chunk_metadata instead.
        // Return a placeholder that will be overwritten.
        None
    }

    fn adjust_chunk_metadata(
        &self,
        kind: &mut ChunkKind,
        name: &str,
        parent: &Option<String>,
        visibility: &mut Option<String>,
    ) {
        // Promote Function to Method when inside a class
        if parent.is_some() && *kind == ChunkKind::Function {
            *kind = ChunkKind::Method;
        }

        // Python visibility: _private, __dunder__, public
        *visibility = if name.starts_with("__") && name.ends_with("__") {
            Some("dunder".into())
        } else if name.starts_with('_') {
            Some("private".into())
        } else {
            Some("public".into())
        };
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        match kind {
            ChunkKind::Function | ChunkKind::Method => content
                .find(':')
                .map(|pos| content[..pos].trim().to_string()),
            ChunkKind::Class => extract_type_signature_to_colon(content),
            _ => None,
        }
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_parent_by_kind(node, source, &["class_definition"], "identifier")
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_python_docstring(node, source).or_else(|| collect_python_comment(node, source))
    }

    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_python_decorators(node, source)
    }
}

/// Public type alias for the Python parser.
pub type PythonParser = BaseParser<PythonConfig>;

impl Default for PythonParser {
    fn default() -> Self {
        Self::new(PythonConfig::new())
    }
}

impl PythonParser {
    /// Create a new Python parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(PythonConfig::new())
    }
}

fn collect_python_docstring(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Python docstrings are INSIDE the function/class body, not before it
    let body = node.child_by_field_name("body")?;
    // body is a "block" node; first child after ":" could be a string expression
    for i in 0..body.child_count() {
        let child = match body.child(i as u32) {
            Some(c) => c,
            None => continue,
        };
        if child.kind() == "expression_statement" {
            let str_node = match child.child(0) {
                Some(n) if n.kind() == "string" => n,
                _ => continue,
            };
            return str_node
                .utf8_text(source)
                .ok()
                .map(std::string::ToString::to_string);
        }
        // Skip newline/indent nodes but stop at non-string statements
        if child.kind() != "comment" {
            break;
        }
    }
    None
}

fn collect_python_decorators(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Check if this function/class is wrapped in a decorated_definition
    let parent = node.parent()?;
    if parent.kind() != "decorated_definition" {
        return None;
    }
    let decorators: Vec<String> = (0..parent.child_count())
        .filter_map(|i| parent.child(i as u32))
        .filter(|c| c.kind() == "decorator")
        .map(|c| c.utf8_text(source).unwrap_or("").to_string())
        .collect();

    if decorators.is_empty() {
        None
    } else {
        Some(decorators.join("\n"))
    }
}

fn collect_python_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Collect preceding # comments (not decorators)
    let mut lines = Vec::new();
    let check_node = if let Some(parent) = node.parent() {
        if parent.kind() == "decorated_definition" {
            parent
        } else {
            node
        }
    } else {
        node
    };
    let mut current = check_node.prev_sibling();
    while let Some(sib) = current {
        if sib.kind() == "comment" {
            lines.push(sib.utf8_text(source).unwrap_or("").to_string());
            current = sib.prev_sibling();
            continue;
        }
        break;
    }
    lines.reverse();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

#[cfg(test)]
#[path = "python_tests.rs"]
mod tests;
