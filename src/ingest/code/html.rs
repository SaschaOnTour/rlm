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

const CHUNK_QUERY_SRC: &str = r#"
    ; Elements with id attribute
    (element
        (start_tag
            (tag_name) @tag_name
            (attribute
                (attribute_name) @attr_name
                (quoted_attribute_value) @id_value
                (#eq? @attr_name "id")))
        ) @element_with_id

    ; Script elements
    (script_element) @script_el

    ; Style elements
    (style_element) @style_el

    ; Doctype
    (doctype) @doctype_el
"#;

const REF_QUERY_SRC: &str = r#"
    ; Class references
    (attribute
        (attribute_name) @_class_attr
        (quoted_attribute_value) @class_value
        (#eq? @_class_attr "class"))

    ; href links
    (attribute
        (attribute_name) @_href_attr
        (quoted_attribute_value) @href_value
        (#eq? @_href_attr "href"))

    ; src references
    (attribute
        (attribute_name) @_src_attr
        (quoted_attribute_value) @src_value
        (#eq? @_src_attr "src"))
"#;

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
                .split(|c: char| c == '"' || c == '\'')
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
mod tests {
    use super::*;
    use crate::ingest::code::CodeParser;
    use crate::models::chunk::RefKind;

    fn parser() -> HtmlParser {
        HtmlParser::create()
    }

    #[test]
    fn parse_html_with_ids() {
        let source = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Test</title>
</head>
<body>
    <div id="app">
        <header id="header">Header</header>
        <main id="content">Content</main>
        <footer id="footer">Footer</footer>
    </div>
</body>
</html>
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "app"));
        assert!(chunks.iter().any(|c| c.ident == "header"));
        assert!(chunks.iter().any(|c| c.ident == "content"));
        assert!(chunks.iter().any(|c| c.ident == "footer"));
    }

    #[test]
    fn parse_html_script_style() {
        let source = r#"
<html>
<head>
    <style>
        body { margin: 0; }
    </style>
</head>
<body>
    <script>
        console.log("Hello");
    </script>
</body>
</html>
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "_script"));
        assert!(chunks.iter().any(|c| c.ident == "_style"));
    }

    #[test]
    fn extract_html_refs() {
        let source = r#"
<div id="app" class="container main">
    <a href="/about">About</a>
    <img src="logo.png" class="logo">
</div>
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let refs = parser().extract_refs(source, &chunks).unwrap();

        // Should find class references
        let class_refs: Vec<_> = refs
            .iter()
            .filter(|r| r.ref_kind == RefKind::TypeUse)
            .collect();
        assert!(
            class_refs.len() >= 3,
            "Should find at least 3 class references"
        );

        // Should find href/src imports
        let import_refs: Vec<_> = refs
            .iter()
            .filter(|r| r.ref_kind == RefKind::Import)
            .collect();
        assert!(import_refs.len() >= 2, "Should find at least 2 import refs");
    }

    #[test]
    fn validate_html_syntax() {
        assert!(parser().validate_syntax("<div>Hello</div>"));
    }

    #[test]
    fn byte_offset_round_trip() {
        let source = r#"
<div id="app">
    <span id="inner">Text</span>
</div>
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        for chunk in &chunks {
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
    fn html5_semantic_elements() {
        let source = r#"
<!DOCTYPE html>
<html>
<body>
    <header id="header">
        <nav id="nav">Navigation</nav>
    </header>
    <main id="main">
        <article id="post">
            <section id="intro">Introduction</section>
        </article>
        <aside id="sidebar">Sidebar</aside>
    </main>
    <footer id="footer">Footer</footer>
</body>
</html>
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "header"));
        assert!(chunks.iter().any(|c| c.ident == "nav"));
        assert!(chunks.iter().any(|c| c.ident == "main"));
        assert!(chunks.iter().any(|c| c.ident == "post"));
        assert!(chunks.iter().any(|c| c.ident == "sidebar"));
    }

    #[test]
    fn parse_with_quality_clean() {
        let source = "<div>Hello</div>";
        let result = parser().parse_with_quality(source, 1).unwrap();
        assert!(result.quality.is_complete());
    }
}
