//! Base parser infrastructure for tree-sitter-based code parsing.
//!
//! This module provides a generic `BaseParser` that implements common parsing logic,
//! reducing code duplication across the 6 language-specific parsers.

use std::collections::HashSet;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

/// Build the (Language, chunk Query, ref Query) triple shared by every parser config.
///
/// Panics with a descriptive message if either query fails to compile.
pub fn build_language_config(
    language: Language,
    chunk_query_src: &str,
    ref_query_src: &str,
    lang_name: &str,
) -> (Language, Query, Query) {
    let chunk_query = Query::new(&language, chunk_query_src)
        .unwrap_or_else(|e| panic!("{lang_name} chunk query must compile: {e}"));
    let ref_query = Query::new(&language, ref_query_src)
        .unwrap_or_else(|e| panic!("{lang_name} ref query must compile: {e}"));
    (language, chunk_query, ref_query)
}

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

    /// Check if a capture name represents an import declaration.
    /// Override this for languages with multiple import capture names (e.g., JS `require_decl`).
    fn is_import_capture(&self, capture_name: &str) -> bool {
        capture_name == self.import_capture_name()
    }

    /// Filter a ref capture based on the capture text. Return `false` to skip this reference.
    /// Default: accept all captures.
    fn filter_ref_capture(&self, _capture_name: &str, _text: &str) -> bool {
        true
    }

    /// Transform the reference target text before storing it.
    /// Default: return text as-is.
    fn transform_ref_text(&self, _capture_name: &str, text: &str) -> String {
        text.to_string()
    }

    /// Returns true if this language uses deduplication (some languages emit duplicate matches).
    fn needs_deduplication(&self) -> bool {
        false
    }

    /// Optional: adjust chunk kind and visibility after parent is found.
    /// For example, Python promotes Function to Method when inside a class,
    /// and derives visibility from the identifier name rather than content.
    fn adjust_chunk_metadata(
        &self,
        _kind: &mut ChunkKind,
        _name: &str,
        _parent: &Option<String>,
        _visibility: &mut Option<String>,
    ) {
        // Default: no adjustment
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

    /// Optional: return a UI context hint for chunks (e.g., "ui" for CSS/HTML).
    fn ui_ctx(&self) -> Option<String> {
        None
    }

    /// Expand a ref capture into one or more (target, RefKind) pairs.
    /// Override this when a single capture may produce multiple references
    /// (e.g., HTML class attributes with space-separated values).
    /// Default: delegates to `map_ref_capture` + `transform_ref_text`.
    fn expand_ref_capture(&self, capture_name: &str, text: &str) -> Vec<(String, RefKind)> {
        match self.map_ref_capture(capture_name) {
            Some(kind) => {
                if !self.filter_ref_capture(capture_name, text) {
                    return Vec::new();
                }
                let target = self.transform_ref_text(capture_name, text);
                vec![(target, kind)]
            }
            None => Vec::new(),
        }
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

impl ChunkCaptureResult {
    /// Create a named capture result (not a definition node).
    pub fn name(name: String, kind: ChunkKind) -> Self {
        Self {
            name,
            kind,
            is_definition_node: false,
        }
    }

    /// Create a definition-node capture result (no name, `ChunkKind::Other("def")`).
    pub fn definition() -> Self {
        Self {
            name: String::new(),
            kind: ChunkKind::Other("def".into()),
            is_definition_node: true,
        }
    }

    /// Create a named definition-node capture result.
    pub fn named_definition(name: String, kind: ChunkKind) -> Self {
        Self {
            name,
            kind,
            is_definition_node: true,
        }
    }
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

    /// Extract chunks from a parsed tree (integration function).
    ///
    /// Orchestrates match processing, deduplication, post-processing, and
    /// import-chunk creation by delegating to operation helpers in `base_ops`.
    fn extract_chunks_from_tree(
        &self,
        tree: &Tree,
        source_bytes: &[u8],
        file_id: i64,
    ) -> Vec<Chunk> {
        use super::base_ops::{
            build_chunk_from_match_data, build_import_chunk, process_query_match, QueryMatchResult,
        };

        let mut chunks = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(self.config.chunk_query(), tree.root_node(), source_bytes);
        let mut import_decls: Vec<tree_sitter::Node> = Vec::new();
        let mut seen: HashSet<(String, u32)> = if self.config.needs_deduplication() {
            HashSet::new()
        } else {
            HashSet::with_capacity(0)
        };

        while let Some(m) = matches.next() {
            let match_result = process_query_match(m, source_bytes, &self.config, tree.root_node());
            match match_result {
                QueryMatchResult::Import(node) => {
                    import_decls.push(node);
                }
                QueryMatchResult::Chunk(data) => {
                    let built = build_chunk_from_match_data(
                        &data,
                        source_bytes,
                        file_id,
                        &self.config,
                        &mut seen,
                    );
                    chunks.extend(built);
                }
                QueryMatchResult::Skip => {}
            }
        }

        self.config
            .post_process_chunks(&mut chunks, tree, source_bytes, file_id);

        let import_chunk = build_import_chunk(&import_decls, source_bytes, file_id);
        chunks.extend(import_chunk);

        chunks
    }
}

impl<C: LanguageConfig> BaseParser<C> {
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

                let expanded = self.config.expand_ref_capture(cap_name, &text);
                if expanded.is_empty() {
                    continue;
                }

                let line = pos.row as u32 + 1;
                let chunk_id = chunks
                    .iter()
                    .find(|c| line >= c.start_line && line <= c.end_line)
                    .map_or(0, |c| c.id);

                for (target, ref_kind) in expanded {
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

// Re-export all helpers from the dedicated helpers module for backward compatibility.
pub use super::helpers::{
    collect_prev_siblings, collect_prev_siblings_filtered_skip, extract_keyword_visibility,
    extract_type_signature, extract_type_signature_to, extract_type_signature_to_brace,
    extract_type_signature_to_colon, find_parent_by_kind, first_child_text_by_kind,
    SiblingCollectConfig,
};

#[cfg(test)]
pub use super::helpers::{extract_signature_to_brace, extract_signature_to_colon};
