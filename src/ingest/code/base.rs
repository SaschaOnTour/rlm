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
                if self.config.is_import_capture(cap_name) {
                    is_import_decl = true;
                    import_decls.push(cap.node);
                    continue;
                }

                // Map the capture to chunk info
                if let Some(result) = self.config.map_chunk_capture(cap_name, text) {
                    if result.is_definition_node {
                        node = cap.node;
                        // Some captures are both the definition node AND provide name/kind
                        if !result.name.is_empty() {
                            name = result.name;
                            kind = result.kind;
                        }
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

            let mut visibility = self.config.extract_visibility(&content);
            self.config
                .adjust_chunk_metadata(&mut kind, &name, &parent, &mut visibility);
            let signature = self.config.extract_signature(&content, &kind);
            let doc_comment = self.config.collect_doc_comment(node, source_bytes);
            let attributes = self.config.collect_attributes(node, source_bytes);

            chunks.push(Chunk {
                start_line,
                end_line: end.row as u32 + 1,
                start_byte: node.start_byte() as u32,
                end_byte: node.end_byte() as u32,
                kind,
                ident: name,
                parent,
                signature,
                visibility,
                ui_ctx: self.config.ui_ctx(),
                doc_comment,
                attributes,
                content,
                ..Chunk::stub(file_id)
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
                start_line: start_line as u32 + 1,
                end_line: end_line as u32 + 1,
                start_byte: start_byte as u32,
                end_byte: end_byte as u32,
                kind: ChunkKind::Other("imports".into()),
                ident: "_imports".to_string(),
                content,
                ..Chunk::stub(file_id)
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

// =============================================================================
// Helper functions for common parsing patterns
// =============================================================================

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

/// Extract type signature up to the given `delimiter`, or fall back to the first line.
///
/// Used by C#, Go, Java, and PHP (delimiter `'{'`) and Python (delimiter `':'`).
#[must_use]
pub fn extract_type_signature_to(content: &str, delimiter: char) -> Option<String> {
    if let Some(pos) = content.find(delimiter) {
        let sig = content[..pos].trim();
        Some(sig.to_string())
    } else {
        content.lines().next().map(|s| s.trim().to_string())
    }
}

/// Convenience wrapper: extract type signature up to `{`.
#[must_use]
pub fn extract_type_signature_to_brace(content: &str) -> Option<String> {
    extract_type_signature_to(content, '{')
}

/// Convenience wrapper: extract type signature up to `:`.
#[must_use]
pub fn extract_type_signature_to_colon(content: &str) -> Option<String> {
    extract_type_signature_to(content, ':')
}

/// Extract keyword-based visibility from content.
///
/// Scans for common visibility keywords at the start of the content.
/// `default_visibility` is returned when no keyword matches (language-dependent).
/// `extra_keywords` allows adding language-specific keywords like `"internal"` for C#.
#[must_use]
pub fn extract_keyword_visibility(
    content: &str,
    default_visibility: &str,
    extra_keywords: &[(&str, &str)],
) -> Option<String> {
    let trimmed = content.trim_start();
    // Check extra keywords first (they may be more specific, e.g. "pub(crate)" before "pub")
    for &(keyword, value) in extra_keywords {
        if trimmed.starts_with(keyword) {
            return Some(value.into());
        }
    }
    if trimmed.starts_with("public") {
        Some("public".into())
    } else if trimmed.starts_with("protected") {
        Some("protected".into())
    } else if trimmed.starts_with("private") {
        Some("private".into())
    } else {
        Some(default_visibility.into())
    }
}

/// Walk up the tree-sitter tree to find a parent node matching one of `parent_kinds`,
/// then extract the identifier from its child matching `ident_kind`.
///
/// Used by C#, Java, PHP, Python, and Rust to find enclosing class/struct/impl names.
#[must_use]
pub fn find_parent_by_kind(
    node: tree_sitter::Node,
    source: &[u8],
    parent_kinds: &[&str],
    ident_kind: &str,
) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent_kinds.contains(&parent.kind()) {
            for i in 0..parent.child_count() {
                if let Some(child) = parent.child(i as u32) {
                    if child.kind() == ident_kind {
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

/// Controls where prefix-based filtering is applied in [`collect_prev_siblings_core`].
pub enum PrefixFilter<'a> {
    /// Filter on `collect_kinds`: only collect nodes whose text matches a prefix.
    /// A matching kind that fails the prefix check **stops** the walk.
    OnCollect(&'a [&'a str]),
    /// Filter on `skip_kinds`: only skip nodes whose text matches a prefix.
    /// A skip-kind node that fails the prefix check **stops** the walk.
    OnSkip(&'a [&'a str]),
    /// No prefix filtering at all.
    None,
}

/// Walk previous siblings of `node`, collecting text from siblings whose
/// `kind()` is in `collect_kinds` and skipping over siblings whose `kind()`
/// is in `skip_kinds`.  Any other sibling kind stops the walk.
///
/// `prefix_filter` controls optional prefix-based filtering (see [`PrefixFilter`]).
///
/// When `multi` is `true`, all consecutive matching siblings are accumulated
/// (e.g. consecutive `///` doc-comment lines).  When `false`, at most one
/// match is returned (e.g. a single `/** ... */` block).
///
/// Results are returned in source order (reversed from walk order).
#[must_use]
fn collect_prev_siblings_core(
    node: tree_sitter::Node,
    source: &[u8],
    collect_kinds: &[&str],
    skip_kinds: &[&str],
    prefix_filter: &PrefixFilter<'_>,
    multi: bool,
) -> Option<String> {
    let mut items = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        let kind = sib.kind();
        if collect_kinds.contains(&kind) {
            let text = sib.utf8_text(source).unwrap_or("");
            if let PrefixFilter::OnCollect(prefixes) = prefix_filter {
                if !prefixes.is_empty() && !prefixes.iter().any(|p| text.starts_with(p)) {
                    break;
                }
            }
            items.push(text.to_string());
            if !multi {
                break;
            }
            current = sib.prev_sibling();
            continue;
        }
        if skip_kinds.contains(&kind) {
            if let PrefixFilter::OnSkip(prefixes) = prefix_filter {
                let text = sib.utf8_text(source).unwrap_or("");
                if !prefixes.iter().any(|p| text.starts_with(p)) {
                    break;
                }
            }
            current = sib.prev_sibling();
            continue;
        }
        break;
    }
    items.reverse();
    if items.is_empty() {
        None
    } else {
        Some(items.join("\n"))
    }
}

/// Walk previous siblings, collecting nodes in `collect_kinds` and skipping
/// nodes in `skip_kinds`.  When `prefixes` is non-empty, only collected nodes
/// whose text starts with one of the prefixes are kept; a match that fails the
/// prefix check stops the walk.
#[must_use]
pub fn collect_prev_siblings(
    node: tree_sitter::Node,
    source: &[u8],
    collect_kinds: &[&str],
    skip_kinds: &[&str],
    prefixes: &[&str],
    multi: bool,
) -> Option<String> {
    let filter = if prefixes.is_empty() {
        PrefixFilter::None
    } else {
        PrefixFilter::OnCollect(prefixes)
    };
    collect_prev_siblings_core(node, source, collect_kinds, skip_kinds, &filter, multi)
}

/// Like [`collect_prev_siblings`] but skips nodes in `skip_kinds` **only**
/// when their text starts with one of `skip_prefixes`.  If a node matches
/// `skip_kinds` but fails the prefix check the walk stops.
#[must_use]
pub fn collect_prev_siblings_filtered_skip(
    node: tree_sitter::Node,
    source: &[u8],
    collect_kinds: &[&str],
    skip_kinds: &[&str],
    skip_prefixes: &[&str],
    multi: bool,
) -> Option<String> {
    collect_prev_siblings_core(
        node,
        source,
        collect_kinds,
        skip_kinds,
        &PrefixFilter::OnSkip(skip_prefixes),
        multi,
    )
}

#[cfg(test)]
/// Extract signature up to the opening brace.
#[must_use]
pub fn extract_signature_to_brace(content: &str) -> Option<String> {
    content
        .find('{')
        .map(|pos| content[..pos].trim().to_string())
}

#[cfg(test)]
/// Extract Python-style signature (up to colon).
#[must_use]
pub fn extract_signature_to_colon(content: &str) -> Option<String> {
    content
        .find(':')
        .map(|pos| content[..pos].trim().to_string())
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
