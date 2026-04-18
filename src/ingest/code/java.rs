use tree_sitter::{Language, Query};

use crate::infrastructure::parsing::tree_walker::find_parent_by_kind;
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::ingest::code::helpers::{extract_keyword_visibility, extract_type_signature_to_brace};
use crate::models::chunk::{ChunkKind, RefKind};

const CHUNK_QUERY_SRC: &str = include_str!("queries/java/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/java/ref.scm");

pub struct JavaConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl JavaConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_java::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "Java",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for JavaConfig {
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
        "java"
    }

    fn import_capture_name(&self) -> &'static str {
        "import_decl"
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
            "method_name" | "ctor_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Method,
            )),
            "class_def" | "iface_def" | "enum_def" | "method_def" | "ctor_def" => {
                Some(ChunkCaptureResult::definition())
            }
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" => Some(RefKind::Call),
            "import_path" | "import_simple" => Some(RefKind::Import),
            "type_ref" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        extract_keyword_visibility(content, "package", &[])
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        match kind {
            ChunkKind::Method => content
                .find('{')
                .map(|pos| content[..pos].trim().to_string()),
            ChunkKind::Class | ChunkKind::Interface | ChunkKind::Enum => {
                extract_type_signature_to_brace(content)
            }
            _ => None,
        }
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_parent_by_kind(
            node,
            source,
            &["class_declaration", "interface_declaration"],
            "identifier",
        )
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_java_doc_comment(node, source)
    }

    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_java_annotations(node, source)
    }
}

/// Public type alias for the Java parser.
pub type JavaParser = BaseParser<JavaConfig>;

impl Default for JavaParser {
    fn default() -> Self {
        Self::new(JavaConfig::new())
    }
}

impl JavaParser {
    /// Create a new Java parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(JavaConfig::new())
    }
}

fn collect_java_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Check the previous sibling for javadoc or line comments
    if let Some(sib) = node.prev_sibling() {
        if sib.kind() == "block_comment" {
            let text = sib.utf8_text(source).unwrap_or("");
            if text.starts_with("/**") {
                return Some(text.to_string());
            }
        }
        if sib.kind() == "line_comment" {
            // Collect consecutive line comments
            let mut lines = vec![sib.utf8_text(source).unwrap_or("").to_string()];
            let mut prev = sib.prev_sibling();
            while let Some(p) = prev {
                if p.kind() == "line_comment" {
                    lines.push(p.utf8_text(source).unwrap_or("").to_string());
                    prev = p.prev_sibling();
                } else {
                    break;
                }
            }
            lines.reverse();
            return Some(lines.join("\n"));
        }
    }
    None
}

fn collect_java_annotations(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // In Java, annotations are within the modifiers child of the declaration
    let modifiers = (0..node.child_count())
        .filter_map(|i| node.child(i as u32))
        .find(|child| child.kind() == "modifiers")?;

    let annots: Vec<String> = (0..modifiers.child_count())
        .filter_map(|j| modifiers.child(j as u32))
        .filter(|c| c.kind() == "marker_annotation" || c.kind() == "annotation")
        .map(|c| c.utf8_text(source).unwrap_or("").to_string())
        .collect();

    if annots.is_empty() {
        None
    } else {
        Some(annots.join("\n"))
    }
}

#[cfg(test)]
#[path = "java_tests.rs"]
mod tests;
