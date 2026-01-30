//! CSS parser for rlm-cli.
//!
//! Extracts semantic structure from CSS files including:
//! - Rule sets (selectors and their declarations)
//! - Media queries
//! - CSS variables (custom properties)
//! - @import statements
//! - Keyframe animations

use std::collections::HashSet;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::{Result, RlmError};
use crate::ingest::code::CodeParser;
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};

const CHUNK_QUERY_SRC: &str = r"
    ; Rule sets (selectors with declarations)
    (rule_set
        (selectors) @selector) @rule

    ; Media queries
    (media_statement) @media_query

    ; Keyframes
    (keyframes_statement) @keyframe

    ; Import statements
    (import_statement) @import_stmt
";

const REF_QUERY_SRC: &str = r"
    ; Class selectors
    (class_selector
        (class_name) @class_ref)

    ; ID selectors
    (id_selector
        (id_name) @id_ref)
";

pub struct CssParser {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl Default for CssParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CssParser {
    #[must_use]
    pub fn new() -> Self {
        let language: Language = tree_sitter_css::LANGUAGE.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("CSS chunk query must compile");
        let ref_query = Query::new(&language, REF_QUERY_SRC).expect("CSS ref query must compile");
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
                detail: format!("failed to set CSS language: {e}"),
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

        // Collect imports for an imports chunk
        let mut import_stmts: Vec<tree_sitter::Node> = Vec::new();
        // Track seen to avoid duplicates
        let mut seen: HashSet<(String, u32)> = HashSet::new();

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = ChunkKind::Other("rule".into());
            let mut node = tree.root_node();
            let mut is_import = false;

            for cap in m.captures {
                let cap_name = &self.chunk_query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("");

                match *cap_name {
                    "rule" => {
                        node = cap.node;
                    }
                    "selector" => {
                        // Extract the first selector as the name
                        name = text.split(',').next().unwrap_or(text).trim().to_string();
                        kind = ChunkKind::Other("rule".into());
                    }
                    "media_query" => {
                        node = cap.node;
                        // Extract @media query condition
                        name = extract_media_name(text);
                        kind = ChunkKind::Other("media".into());
                    }
                    "keyframe" => {
                        node = cap.node;
                        // Extract keyframe name from content
                        name = extract_keyframe_name(text);
                        kind = ChunkKind::Other("keyframes".into());
                    }
                    "import_stmt" => {
                        is_import = true;
                        import_stmts.push(cap.node);
                    }
                    _ => {}
                }
            }

            if is_import {
                continue;
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

            // Signature is the selector or @-rule header
            let signature = content
                .find('{')
                .map(|pos| content[..pos].trim().to_string());

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

        // Create imports chunk
        if !import_stmts.is_empty() {
            let start_line = import_stmts
                .iter()
                .map(|n| n.start_position().row)
                .min()
                .unwrap_or(0);
            let end_line = import_stmts
                .iter()
                .map(|n| n.end_position().row)
                .max()
                .unwrap_or(0);
            let start_byte = import_stmts
                .iter()
                .map(tree_sitter::Node::start_byte)
                .min()
                .unwrap_or(0);
            let end_byte = import_stmts
                .iter()
                .map(tree_sitter::Node::end_byte)
                .max()
                .unwrap_or(0);

            let content: String = import_stmts
                .iter()
                .filter_map(|n| n.utf8_text(source_bytes).ok())
                .collect::<Vec<_>>()
                .join("\n");

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line: start_line as u32 + 1,
                end_line: end_line as u32 + 1,
                start_byte: start_byte as u32,
                end_byte: end_byte as u32,
                kind: ChunkKind::Other("imports".into()),
                ident: "_imports".to_string(),
                parent: None,
                signature: None,
                visibility: None,
                ui_ctx: None,
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

                let ref_kind = match *cap_name {
                    "class_ref" | "id_ref" => RefKind::TypeUse,
                    _ => continue,
                };

                // Clean up the text
                let target = text.trim_matches('"').trim_matches('\'').to_string();

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

impl CodeParser for CssParser {
    fn language(&self) -> &'static str {
        "css"
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

    fn parser() -> CssParser {
        CssParser::new()
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
