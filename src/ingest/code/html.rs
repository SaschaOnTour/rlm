//! HTML parser for rlm.
//!
//! Extracts semantic structure from HTML documents including:
//! - Elements with IDs
//! - Elements with class attributes
//! - Script and style blocks
//! - Template markers (Vue, Angular)

use std::collections::HashSet;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::{Result, RlmError};
use crate::ingest::code::CodeParser;
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};

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

pub struct HtmlParser {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl Default for HtmlParser {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlParser {
    #[must_use]
    pub fn new() -> Self {
        let language: Language = tree_sitter_html::LANGUAGE.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("HTML chunk query must compile");
        let ref_query = Query::new(&language, REF_QUERY_SRC).expect("HTML ref query must compile");
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }

    fn make_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| RlmError::Parse {
                path: String::new(),
                detail: format!("failed to set HTML language: {e}"),
            })?;
        Ok(parser)
    }

    fn extract_chunks_from_tree(
        &self,
        tree: &Tree,
        source_bytes: &[u8],
        file_id: i64,
    ) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.chunk_query, tree.root_node(), source_bytes);

        // Track seen to avoid duplicates
        let mut seen: HashSet<(String, u32)> = HashSet::new();

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = ChunkKind::Other("element".into());
            let mut node = tree.root_node();
            let mut tag_name = String::new();

            for cap in m.captures {
                let cap_name = &self.chunk_query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("");

                match *cap_name {
                    "element_with_id" => {
                        node = cap.node;
                    }
                    "tag_name" => {
                        tag_name = text.to_string();
                    }
                    "id_value" => {
                        // Remove quotes
                        name = text.trim_matches('"').trim_matches('\'').to_string();
                        kind = ChunkKind::Other("element".into());
                    }
                    "script_el" => {
                        node = cap.node;
                        name = "_script".to_string();
                        kind = ChunkKind::Other("script".into());
                    }
                    "style_el" => {
                        node = cap.node;
                        name = "_style".to_string();
                        kind = ChunkKind::Other("style".into());
                    }
                    "doctype_el" => {
                        node = cap.node;
                        name = "_doctype".to_string();
                        kind = ChunkKind::Other("doctype".into());
                    }
                    _ => {}
                }
            }

            if name.is_empty() {
                continue;
            }

            let start = node.start_position();
            let start_line = start.row as u32 + 1;

            // Skip duplicates
            let key = (name.clone(), start_line);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            let content = node.utf8_text(source_bytes).unwrap_or("").to_string();
            let end = node.end_position();

            // Create a signature with the tag name
            let signature = if tag_name.is_empty() {
                None
            } else {
                Some(format!("<{tag_name} id=\"{name}\">"))
            };

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line,
                end_line: end.row as u32 + 1,
                start_byte: node.start_byte() as u32,
                end_byte: node.end_byte() as u32,
                kind,
                ident: name,
                parent: None,
                signature,
                visibility: None,
                ui_ctx: Some("ui".into()),
                doc_comment: None,
                attributes: None,
                content,
            });
        }

        chunks
    }

    fn extract_refs_from_tree(
        &self,
        tree: &Tree,
        source_bytes: &[u8],
        chunks: &[Chunk],
    ) -> Vec<Reference> {
        let mut refs = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.ref_query, tree.root_node(), source_bytes);

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let cap_name = &self.ref_query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("").to_string();
                let pos = cap.node.start_position();

                let (ref_kind, target) = match *cap_name {
                    "class_value" => {
                        // Classes can have multiple space-separated values
                        let classes = text.trim_matches('"').trim_matches('\'');
                        for class in classes.split_whitespace() {
                            let line = pos.row as u32 + 1;
                            let chunk_id = chunks
                                .iter()
                                .find(|c| line >= c.start_line && line <= c.end_line)
                                .map_or(0, |c| c.id);
                            refs.push(Reference {
                                id: 0,
                                chunk_id,
                                target_ident: class.to_string(),
                                ref_kind: RefKind::TypeUse,
                                line,
                                col: pos.column as u32,
                            });
                        }
                        continue;
                    }
                    "href_value" | "src_value" => (
                        RefKind::Import,
                        text.trim_matches('"').trim_matches('\'').to_string(),
                    ),
                    _ => continue,
                };

                let line = pos.row as u32 + 1;
                let chunk_id = chunks
                    .iter()
                    .find(|c| line >= c.start_line && line <= c.end_line)
                    .map_or(0, |c| c.id);

                refs.push(Reference {
                    id: 0,
                    chunk_id,
                    target_ident: target,
                    ref_kind,
                    line,
                    col: pos.column as u32,
                });
            }
        }

        refs
    }
}

impl CodeParser for HtmlParser {
    fn language(&self) -> &'static str {
        "html"
    }

    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let mut parser = self.make_parser()?;
        let tree = parser.parse(source, None).ok_or_else(|| RlmError::Parse {
            path: String::new(),
            detail: "tree-sitter parse returned None".into(),
        })?;
        Ok(self.extract_chunks_from_tree(&tree, source.as_bytes(), file_id))
    }

    fn extract_refs(&self, source: &str, chunks: &[Chunk]) -> Result<Vec<Reference>> {
        let mut parser = self.make_parser()?;
        let tree = parser.parse(source, None).ok_or_else(|| RlmError::Parse {
            path: String::new(),
            detail: "tree-sitter parse returned None".into(),
        })?;
        Ok(self.extract_refs_from_tree(&tree, source.as_bytes(), chunks))
    }

    fn parse_chunks_and_refs(
        &self,
        source: &str,
        file_id: i64,
    ) -> Result<(Vec<Chunk>, Vec<Reference>)> {
        let mut parser = self.make_parser()?;
        let tree = parser.parse(source, None).ok_or_else(|| RlmError::Parse {
            path: String::new(),
            detail: "tree-sitter parse returned None".into(),
        })?;
        let source_bytes = source.as_bytes();
        let chunks = self.extract_chunks_from_tree(&tree, source_bytes, file_id);
        let refs = self.extract_refs_from_tree(&tree, source_bytes, &chunks);
        Ok((chunks, refs))
    }

    fn validate_syntax(&self, source: &str) -> bool {
        let mut parser = match self.make_parser() {
            Ok(p) => p,
            Err(_) => return false,
        };
        match parser.parse(source, None) {
            Some(tree) => !tree.root_node().has_error(),
            None => false,
        }
    }

    fn parse_with_quality(
        &self,
        source: &str,
        file_id: i64,
    ) -> Result<crate::ingest::code::ParseResult> {
        use crate::ingest::code::{find_error_lines, ParseQuality, ParseResult};

        let mut parser = self.make_parser()?;
        let tree = parser.parse(source, None).ok_or_else(|| RlmError::Parse {
            path: String::new(),
            detail: "tree-sitter parse returned None".into(),
        })?;
        let source_bytes = source.as_bytes();
        let chunks = self.extract_chunks_from_tree(&tree, source_bytes, file_id);
        let refs = self.extract_refs_from_tree(&tree, source_bytes, &chunks);

        let quality = if tree.root_node().has_error() {
            let error_lines = find_error_lines(tree.root_node());
            ParseQuality::Partial {
                error_count: error_lines.len(),
                error_lines,
            }
        } else {
            ParseQuality::Complete
        };

        Ok(ParseResult {
            chunks,
            refs,
            quality,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> HtmlParser {
        HtmlParser::new()
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
