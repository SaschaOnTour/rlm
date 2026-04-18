//! CSS parser for rlm.
//!
//! Extracts semantic structure from CSS files including:
//! - Rule sets (selectors and their declarations)
//! - Media queries
//! - CSS variables (custom properties)
//! - @import statements
//! - Keyframe animations

use tree_sitter::{Language, Query};

use crate::ingest::code::base::{
    build_language_config, BaseParser, ChunkCaptureResult, LanguageConfig,
};
use crate::models::chunk::{ChunkKind, RefKind};

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
mod tests {
    use super::*;
    use crate::ingest::code::CodeParser;
    use crate::models::chunk::ChunkKind;

    fn parser() -> CssParser {
        CssParser::create()
    }

    #[test]
    fn parse_css_rules() {
        let source = r#"
.container {
    max-width: 1200px;
    margin: 0 auto;
}

#header {
    background: white;
}

body {
    margin: 0;
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == ".container"));
        assert!(chunks.iter().any(|c| c.ident == "#header"));
        assert!(chunks.iter().any(|c| c.ident == "body"));
    }

    #[test]
    fn parse_css_media_queries() {
        let source = r#"
@media (min-width: 768px) {
    .container {
        width: 750px;
    }
}

@media screen and (max-width: 480px) {
    .mobile-only {
        display: block;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let media_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Other("media".into()))
            .collect();
        assert!(
            media_chunks.len() >= 2,
            "Should find 2 media queries, got {:?}",
            media_chunks
        );
    }

    #[test]
    fn parse_css_keyframes() {
        let source = r#"
@keyframes fadeIn {
    from {
        opacity: 0;
    }
    to {
        opacity: 1;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "fadeIn"));
    }

    #[test]
    fn extract_css_refs() {
        let source = r#"
.header .nav-item {
    color: blue;
}

#main .content {
    padding: 20px;
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let refs = parser().extract_refs(source, &chunks).unwrap();

        // Should find class references
        let class_refs: Vec<_> = refs
            .iter()
            .filter(|r| r.target_ident == "header" || r.target_ident == "nav-item")
            .collect();
        assert!(!class_refs.is_empty(), "Should find class references");
    }

    #[test]
    fn css_variables() {
        let source = r#"
:root {
    --primary-color: #007bff;
    --secondary-color: #6c757d;
}

.button {
    background: var(--primary-color);
    color: white;
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == ":root"));
        assert!(chunks.iter().any(|c| c.ident == ".button"));
        // CSS variable references require more complex parsing not currently supported
    }

    #[test]
    fn validate_css_syntax() {
        assert!(parser().validate_syntax(".test { color: red; }"));
    }

    #[test]
    fn byte_offset_round_trip() {
        let source = r#"
.container {
    width: 100%;
}

#header {
    background: blue;
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        for chunk in &chunks {
            if chunk.ident == "_imports" {
                continue;
            }
            let reconstructed = &source[chunk.start_byte as usize..chunk.end_byte as usize];
            assert_eq!(
                reconstructed, chunk.content,
                "Byte offset reconstruction failed for chunk '{}'",
                chunk.ident
            );
        }
    }

    #[test]
    fn empty_file() {
        let chunks = parser().parse_chunks("", 1).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn parse_with_quality_clean() {
        let source = ".test { color: red; }";
        let result = parser().parse_with_quality(source, 1).unwrap();
        assert!(result.quality.is_complete());
    }

    #[test]
    fn css_nesting() {
        // Modern CSS nesting (2023 spec)
        let source = r#"
.card {
    padding: 20px;

    & .title {
        font-size: 18px;
    }

    &:hover {
        background: gray;
    }
}
"#;
        // This may or may not work depending on tree-sitter-css support
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == ".card"));
    }
}
