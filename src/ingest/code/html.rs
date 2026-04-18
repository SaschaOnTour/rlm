//! HTML parser for rlm.
//!
//! Extracts semantic structure from HTML documents including:
//! - Elements with IDs
//! - Elements with class attributes
//! - Script and style blocks
//! - Template markers (Vue, Angular)

use tree_sitter::{Language, Query};

use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::models::chunk::{ChunkKind, RefKind};

const CHUNK_QUERY_SRC: &str = include_str!("queries/html/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/html/ref.scm");

pub struct HtmlConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl HtmlConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_html::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "HTML",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for HtmlConfig {
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
        "html"
    }

    fn import_capture_name(&self) -> &'static str {
        // HTML doesn't have a dedicated import capture; use a name that won't match
        "_no_imports"
    }

    fn needs_deduplication(&self) -> bool {
        true
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "element_with_id" => Some(ChunkCaptureResult::named_definition(
                String::new(),
                ChunkKind::Other("element".into()),
            )),
            "tag_name" => {
                // Tag name is stored but the actual ident comes from id_value
                // We just note it for signature construction; not the name.
                // Returning None here because tag_name alone doesn't set the chunk name.
                None
            }
            "id_value" => {
                // Remove quotes from the id value
                let name = text.trim_matches('"').trim_matches('\'').to_string();
                Some(ChunkCaptureResult::name(
                    name,
                    ChunkKind::Other("element".into()),
                ))
            }
            "script_el" => Some(ChunkCaptureResult::named_definition(
                "_script".to_string(),
                ChunkKind::Other("script".into()),
            )),
            "style_el" => Some(ChunkCaptureResult::named_definition(
                "_style".to_string(),
                ChunkKind::Other("style".into()),
            )),
            "doctype_el" => Some(ChunkCaptureResult::named_definition(
                "_doctype".to_string(),
                ChunkKind::Other("doctype".into()),
            )),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "class_value" => Some(RefKind::TypeUse),
            "href_value" | "src_value" => Some(RefKind::Import),
            _ => None,
        }
    }

    fn expand_ref_capture(&self, capture_name: &str, text: &str) -> Vec<(String, RefKind)> {
        match capture_name {
            "class_value" => {
                // Classes can have multiple space-separated values
                let classes = text.trim_matches('"').trim_matches('\'');
                classes
                    .split_whitespace()
                    .map(|class| (class.to_string(), RefKind::TypeUse))
                    .collect()
            }
            "href_value" | "src_value" => {
                let target = text.trim_matches('"').trim_matches('\'').to_string();
                vec![(target, RefKind::Import)]
            }
            _ => Vec::new(),
        }
    }

    fn extract_visibility(&self, _content: &str) -> Option<String> {
        None // HTML has no visibility modifiers
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        // For elements with id, extract the opening tag as signature
        if *kind == ChunkKind::Other("element".into()) {
            extract_html_id_signature(content)
        } else {
            None
        }
    }

    fn find_parent(&self, _node: tree_sitter::Node, _source: &[u8]) -> Option<String> {
        None
    }

    fn collect_doc_comment(&self, _node: tree_sitter::Node, _source: &[u8]) -> Option<String> {
        None
    }

    fn collect_attributes(&self, _node: tree_sitter::Node, _source: &[u8]) -> Option<String> {
        None
    }

    fn ui_ctx(&self) -> Option<String> {
        Some("ui".into())
    }
}

/// Public type alias for the HTML parser.
pub type HtmlParser = BaseParser<HtmlConfig>;

impl Default for HtmlParser {
    fn default() -> Self {
        Self::new(HtmlConfig::new())
    }
}

impl HtmlParser {
    /// Create a new HTML parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(HtmlConfig::new())
    }
}

/// Extract signature for HTML elements with id, e.g. `<div id="app">`.
fn extract_html_id_signature(content: &str) -> Option<String> {
    // Find the tag name and id from the content
    if let Some(tag_end) = content.find('>') {
        let open_tag = &content[..=tag_end];
        // Extract tag name
        let tag_name = open_tag
            .trim_start_matches('<')
            .split_whitespace()
            .next()
            .unwrap_or("");
        // Extract id value
        if let Some(id_start) = open_tag.find("id=") {
            let after_id = &open_tag[id_start + 3..];
            let id_val = after_id
                .trim_start_matches('"')
                .trim_start_matches('\'')
                .split(['"', '\''])
                .next()
                .unwrap_or("");
            if !tag_name.is_empty() && !id_val.is_empty() {
                return Some(format!("<{tag_name} id=\"{id_val}\">"));
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "html_tests.rs"]
mod tests;
