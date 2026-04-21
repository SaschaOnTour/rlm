//! CSS parser for rlm.
//!
//! Extracts semantic structure from CSS files including:
//! - Rule sets (selectors and their declarations)
//! - Media queries
//! - CSS variables (custom properties)
//! - @import statements
//! - Keyframe animations

use tree_sitter::{Language, Query};

use crate::domain::chunk::{ChunkKind, RefKind};
use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};

const CHUNK_QUERY_SRC: &str = include_str!("queries/css/chunk.scm");

const REF_QUERY_SRC: &str = include_str!("queries/css/ref.scm");

pub struct CssConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl CssConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_css::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "CSS",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for CssConfig {
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
        "css"
    }

    fn import_capture_name(&self) -> &'static str {
        "import_stmt"
    }

    fn needs_deduplication(&self) -> bool {
        true
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "selector" => {
                let name = text.split(',').next().unwrap_or(text).trim().to_string();
                Some(ChunkCaptureResult::name(
                    name,
                    ChunkKind::Other("rule".into()),
                ))
            }
            "rule" => Some(ChunkCaptureResult::definition()),
            "media_query" => Some(ChunkCaptureResult::named_definition(
                extract_media_name(text),
                ChunkKind::Other("media".into()),
            )),
            "keyframe" => Some(ChunkCaptureResult::named_definition(
                extract_keyframe_name(text),
                ChunkKind::Other("keyframes".into()),
            )),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "class_ref" | "id_ref" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn transform_ref_text(&self, _capture_name: &str, text: &str) -> String {
        text.trim_matches('"').trim_matches('\'').to_string()
    }

    fn extract_visibility(&self, _content: &str) -> Option<String> {
        None // CSS has no visibility modifiers
    }

    fn extract_signature(&self, content: &str, _kind: &ChunkKind) -> Option<String> {
        content
            .find('{')
            .map(|pos| content[..pos].trim().to_string())
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

/// Public type alias for the CSS parser.
pub type CssParser = BaseParser<CssConfig>;

impl Default for CssParser {
    fn default() -> Self {
        Self::new(CssConfig::new())
    }
}

impl CssParser {
    /// Create a new CSS parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(CssConfig::new())
    }
}

fn extract_media_name(content: &str) -> String {
    // Extract the media query part between @media and {
    if let Some(start) = content.find("@media") {
        let after_media = &content[start + 6..];
        if let Some(brace) = after_media.find('{') {
            return format!("@media {}", after_media[..brace].trim());
        }
    }
    "@media".to_string()
}

fn extract_keyframe_name(content: &str) -> String {
    // Extract the keyframe name between @keyframes and {
    if let Some(start) = content.find("@keyframes") {
        let after_kf = &content[start + 10..];
        if let Some(brace) = after_kf.find('{') {
            return after_kf[..brace].trim().to_string();
        }
    }
    "_keyframes".to_string()
}

#[cfg(test)]
#[path = "css_tests.rs"]
mod tests;
