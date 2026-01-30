//! Base parser infrastructure for tree-sitter-based code parsing.
//!
//! This module provides a generic `BaseParser` that implements common parsing logic,
//! reducing code duplication across the 6 language-specific parsers.

use std::collections::HashSet;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::{Result, RlmError};
use crate::ingest::code::{find_error_lines, CodeParser, ParseQuality, ParseResult};
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};

/// Configuration trait for language-specific parsing behavior.
///
/// Implement this trait to define how a specific language should be parsed.
/// The `BaseParser` uses this configuration to handle the common parsing logic.
pub trait LanguageConfig: Send + Sync {
    /// Returns the tree-sitter Language for this parser.
    fn language(&self) -> &Language;

    /// Returns the compiled chunk query for extracting code symbols.
    fn chunk_query(&self) -> &Query;

    /// Returns the compiled reference query for extracting call sites/imports.
    fn ref_query(&self) -> &Query;

    /// Returns the human-readable language name (e.g., "rust", "go").
    fn language_name(&self) -> &'static str;

    /// Map a capture name from the chunk query to a `ChunkKind`.
    ///
    /// Returns `None` if this capture should be skipped (e.g., for definition nodes).
    /// Returns `Some((name, kind, is_definition_node))` for valid captures.
    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult>;

    /// Map a capture name from the ref query to a `RefKind`.
    ///
    /// Returns `None` if this capture should be skipped.
    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind>;

    /// Extract visibility from content (e.g., "pub", "private", "public").
    fn extract_visibility(&self, content: &str) -> Option<String>;

    /// Extract signature from content based on the chunk kind.
    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String>;

    /// Find the parent name for nested items (e.g., method in impl block).
    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String>;

    /// Collect doc comments for a node.
    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String>;

    /// Collect attributes/annotations for a node.
    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String>;

    /// Returns the import declaration capture name (e.g., "`use_decl`", "`import_decl`").
    fn import_capture_name(&self) -> &'static str;

    /// Returns true if this language uses deduplication (some languages emit duplicate matches).
    fn needs_deduplication(&self) -> bool {
        false
    }

    /// Optional: post-process chunks after initial extraction (e.g., extract impl methods).
    fn post_process_chunks(
        &self,
        _chunks: &mut Vec<Chunk>,
        _tree: &Tree,
        _source: &[u8],
        _file_id: i64,
    ) {
        // Default: no post-processing
    }

    /// Optional: determine if a function should be skipped (e.g., methods already captured).
    fn should_skip_function(&self, _kind: &ChunkKind, _parent: &Option<String>) -> bool {
        false
    }
}

/// Result from mapping a chunk capture.
pub struct ChunkCaptureResult {
    /// The identifier name extracted from the capture.
    pub name: String,
    /// The kind of chunk this represents.
    pub kind: ChunkKind,
    /// If true, this capture represents the definition node (full content).
    pub is_definition_node: bool,
}

/// Generic base parser that uses a `LanguageConfig` to handle parsing.
pub struct BaseParser<C: LanguageConfig> {
    config: C,
}

impl<C: LanguageConfig> BaseParser<C> {
    /// Create a new base parser with the given configuration.
    pub fn new(config: C) -> Self {
        Self { config }
    }

    /// Create a tree-sitter parser configured for this language.
    fn make_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser
            .set_language(self.config.language())
            .map_err(|e| RlmError::Parse {
                path: String::new(),
                detail: format!(
                    "failed to set {} language: {e}",
                    self.config.language_name()
                ),
            })?;
        Ok(parser)
    }

    /// Extract chunks from a parsed tree.
    fn extract_chunks_from_tree(
        &self,
        tree: &Tree,
        source_bytes: &[u8],
        file_id: i64,
    ) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(self.config.chunk_query(), tree.root_node(), source_bytes);

        // Collect import declarations for an imports chunk
        let mut import_decls: Vec<tree_sitter::Node> = Vec::new();

        // Track seen chunks to avoid duplicates (name + start_line)
        let mut seen: HashSet<(String, u32)> = if self.config.needs_deduplication() {
            HashSet::new()
        } else {
            HashSet::with_capacity(0)
        };

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = ChunkKind::Other("unknown".into());
            let mut node = tree.root_node();
            let mut is_import_decl = false;

            for cap in m.captures {
                let cap_name = &self.config.chunk_query().capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("");

                // Check for import declarations
                if *cap_name == self.config.import_capture_name() {
                    is_import_decl = true;
                    import_decls.push(cap.node);
                    continue;
                }

                // Map the capture to chunk info
                if let Some(result) = self.config.map_chunk_capture(cap_name, text) {
                    if result.is_definition_node {
                        node = cap.node;
                    } else {
                        name = result.name;
                        kind = result.kind;
                    }
                }
            }

            // Skip import declarations - we'll create a single imports chunk
            if is_import_decl {
                continue;
            }

            if name.is_empty() {
                continue;
            }

            let start = node.start_position();
            let start_line = start.row as u32 + 1;

            // Skip duplicates if needed
            if self.config.needs_deduplication() {
                let key = (name.clone(), start_line);
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key);
            }

            let content = node.utf8_text(source_bytes).unwrap_or("").to_string();
            let end = node.end_position();

            let parent = self.config.find_parent(node, source_bytes);

            // Skip functions that should be handled differently (e.g., methods in impl blocks)
            if self.config.should_skip_function(&kind, &parent) {
                continue;
            }

            let visibility = self.config.extract_visibility(&content);
            let signature = self.config.extract_signature(&content, &kind);
            let doc_comment = self.config.collect_doc_comment(node, source_bytes);
            let attributes = self.config.collect_attributes(node, source_bytes);

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line,
                end_line: end.row as u32 + 1,
                start_byte: node.start_byte() as u32,
                end_byte: node.end_byte() as u32,
                kind,
                ident: name,
                parent,
                signature,
                visibility,
                ui_ctx: None,
                doc_comment,
                attributes,
                content,
            });
        }

        // Post-process chunks (e.g., extract impl methods)
        self.config
            .post_process_chunks(&mut chunks, tree, source_bytes, file_id);

        // Create an imports chunk if there are import declarations
        if !import_decls.is_empty() {
            let start_line = import_decls
                .iter()
                .map(|n| n.start_position().row)
                .min()
                .unwrap_or(0);
            let end_line = import_decls
                .iter()
                .map(|n| n.end_position().row)
                .max()
                .unwrap_or(0);
            let start_byte = import_decls
                .iter()
                .map(tree_sitter::Node::start_byte)
                .min()
                .unwrap_or(0);
            let end_byte = import_decls
                .iter()
                .map(tree_sitter::Node::end_byte)
                .max()
                .unwrap_or(0);

            let content: String = import_decls
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

    /// Extract references from a parsed tree.
    fn extract_refs_from_tree(
        &self,
        tree: &Tree,
        source_bytes: &[u8],
        chunks: &[Chunk],
    ) -> Vec<Reference> {
        let mut refs = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(self.config.ref_query(), tree.root_node(), source_bytes);

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let cap_name = &self.config.ref_query().capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("").to_string();
                let pos = cap.node.start_position();

                let ref_kind = match self.config.map_ref_capture(cap_name) {
                    Some(kind) => kind,
                    None => continue,
                };

                let line = pos.row as u32 + 1;
                let chunk_id = chunks
                    .iter()
                    .find(|c| line >= c.start_line && line <= c.end_line)
                    .map_or(0, |c| c.id);

                refs.push(Reference {
                    id: 0,
                    chunk_id,
                    target_ident: text,
                    ref_kind,
                    line,
                    col: pos.column as u32,
                });
            }
        }

        refs
    }
}

impl<C: LanguageConfig> CodeParser for BaseParser<C> {
    fn language(&self) -> &str {
        self.config.language_name()
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

    fn parse_with_quality(&self, source: &str, file_id: i64) -> Result<ParseResult> {
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

// =============================================================================
// Helper functions for common parsing patterns
// =============================================================================

/// Extract signature up to the opening brace.
#[must_use]
pub fn extract_signature_to_brace(content: &str) -> Option<String> {
    content
        .find('{')
        .map(|pos| content[..pos].trim().to_string())
}

/// Extract signature up to the opening brace or semicolon.
#[must_use]
pub fn extract_signature_to_brace_or_semi(content: &str) -> Option<String> {
    if let Some(brace_pos) = content.find('{') {
        Some(content[..brace_pos].trim().to_string())
    } else {
        content
            .find(';')
            .map(|semi_pos| content[..semi_pos].trim().to_string())
    }
}

/// Extract type signature (first line or up to brace).
#[must_use]
pub fn extract_type_signature(content: &str) -> Option<String> {
    if let Some(brace_pos) = content.find('{') {
        let sig = content[..brace_pos].trim();
        // Remove trailing where clauses if too long
        let sig = if let Some(where_pos) = sig.find("\nwhere") {
            sig[..where_pos].trim()
        } else {
            sig
        };
        Some(sig.to_string())
    } else if let Some(semi_pos) = content.find(';') {
        // Unit struct: `pub struct Foo;`
        Some(content[..=semi_pos].trim().to_string())
    } else {
        // Fallback: first line
        content.lines().next().map(|s| s.trim().to_string())
    }
}

/// Extract Python-style signature (up to colon).
#[must_use]
pub fn extract_signature_to_colon(content: &str) -> Option<String> {
    content
        .find(':')
        .map(|pos| content[..pos].trim().to_string())
}

/// Find parent by walking up the tree looking for specific node kinds.
#[must_use]
pub fn find_parent_by_kinds(
    node: tree_sitter::Node,
    source: &[u8],
    parent_kinds: &[&str],
    identifier_kind: &str,
) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent_kinds.contains(&parent.kind()) {
            for i in 0..parent.child_count() {
                if let Some(child) = parent.child(i as u32) {
                    if child.kind() == identifier_kind {
                        return child
                            .utf8_text(source)
                            .ok()
                            .map(std::string::ToString::to_string);
                    }
                }
            }
        }
        current = parent.parent();
    }
    None
}

/// Collect doc comments by walking previous siblings.
#[must_use]
pub fn collect_doc_comments_by_prefix(
    node: tree_sitter::Node,
    source: &[u8],
    comment_kind: &str,
    prefixes: &[&str],
    skip_kind: Option<&str>,
) -> Option<String> {
    let mut lines = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        // Skip specific node kinds (like attribute lists)
        if let Some(skip) = skip_kind {
            if sib.kind() == skip {
                current = sib.prev_sibling();
                continue;
            }
        }
        if sib.kind() == comment_kind {
            let text = sib.utf8_text(source).unwrap_or("");
            if prefixes.iter().any(|p| text.starts_with(p)) {
                lines.push(text.to_string());
                current = sib.prev_sibling();
                continue;
            }
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

/// Collect attributes/annotations by walking previous siblings.
#[must_use]
pub fn collect_attributes_by_kind(
    node: tree_sitter::Node,
    source: &[u8],
    attr_kind: &str,
    skip_comment_prefixes: Option<&[&str]>,
) -> Option<String> {
    let mut attrs = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        if sib.kind() == attr_kind {
            attrs.push(sib.utf8_text(source).unwrap_or("").to_string());
            current = sib.prev_sibling();
            continue;
        }
        // Skip doc comments when collecting attributes
        if sib.kind() == "line_comment" || sib.kind() == "comment" {
            if let Some(prefixes) = skip_comment_prefixes {
                let text = sib.utf8_text(source).unwrap_or("");
                if prefixes.iter().any(|p| text.starts_with(p)) {
                    current = sib.prev_sibling();
                    continue;
                }
            }
        }
        break;
    }
    attrs.reverse();
    if attrs.is_empty() {
        None
    } else {
        Some(attrs.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_signature_to_brace() {
        assert_eq!(
            extract_signature_to_brace("fn main() { }"),
            Some("fn main()".to_string())
        );
        assert_eq!(extract_signature_to_brace("fn main()"), None);
    }

    #[test]
    fn test_extract_type_signature() {
        assert_eq!(
            extract_type_signature("pub struct Foo { }"),
            Some("pub struct Foo".to_string())
        );
        assert_eq!(
            extract_type_signature("pub struct Foo;"),
            Some("pub struct Foo;".to_string())
        );
    }

    #[test]
    fn test_extract_signature_to_colon() {
        assert_eq!(
            extract_signature_to_colon("def foo(x):\n    pass"),
            Some("def foo(x)".to_string())
        );
    }
}
