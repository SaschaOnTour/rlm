//! Operation helpers for `BaseParser::extract_chunks_from_tree`.
//!
//! Extracted from `base.rs` for SRP compliance. Contains the pure-logic
//! and integration functions that process tree-sitter query matches into chunks.

use std::collections::HashSet;

use crate::domain::chunk::{Chunk, ChunkKind};
use crate::ingest::code::base::{ChunkCaptureResult, LanguageConfig};

/// Result of processing a single tree-sitter query match.
pub(crate) enum QueryMatchResult<'a> {
    /// The match was an import declaration.
    Import(tree_sitter::Node<'a>),
    /// The match produced data for a potential chunk.
    Chunk(MatchData<'a>),
    /// The match should be skipped (empty name or import-only).
    Skip,
}

/// Intermediate data extracted from a single query match (operation output).
pub(crate) struct MatchData<'a> {
    pub name: String,
    pub kind: ChunkKind,
    pub node: tree_sitter::Node<'a>,
}

/// Classification of a single capture within a query match.
enum CaptureClassification {
    /// This capture is an import declaration.
    Import,
    /// This capture was mapped to a chunk capture result.
    Mapped(ChunkCaptureResult),
    /// This capture should be skipped (no mapping).
    Unmapped,
}

/// Classify all captures in a query match (integration: calls config methods).
///
/// Returns a list of `(classification, node)` pairs by calling `config.is_import_capture`
/// and `config.map_chunk_capture` for each capture.
fn classify_captures<'tree, C: LanguageConfig>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    source_bytes: &[u8],
    config: &C,
) -> Vec<(CaptureClassification, tree_sitter::Node<'tree>)> {
    m.captures
        .iter()
        .map(|cap| {
            let cap_name = &config.chunk_query().capture_names()[cap.index as usize];
            let text = cap.node.utf8_text(source_bytes).unwrap_or("");

            let classification = if config.is_import_capture(cap_name) {
                CaptureClassification::Import
            } else {
                match config.map_chunk_capture(cap_name, text) {
                    Some(result) => CaptureClassification::Mapped(result),
                    None => CaptureClassification::Unmapped,
                }
            };

            (classification, cap.node)
        })
        .collect()
}

/// Assemble a `QueryMatchResult` from pre-classified captures (operation: logic only).
fn assemble_match_result<'tree>(
    classifications: Vec<(CaptureClassification, tree_sitter::Node<'tree>)>,
    root_node: tree_sitter::Node<'tree>,
) -> QueryMatchResult<'tree> {
    let mut name = String::new();
    let mut kind = ChunkKind::Other("unknown".into());
    let mut node = root_node;

    for (classification, cap_node) in classifications {
        match classification {
            CaptureClassification::Import => {
                return QueryMatchResult::Import(cap_node);
            }
            CaptureClassification::Mapped(result) => {
                if result.is_definition_node {
                    node = cap_node;
                    if !result.name.is_empty() {
                        name = result.name;
                        kind = result.kind;
                    }
                } else {
                    name = result.name;
                    kind = result.kind;
                }
            }
            CaptureClassification::Unmapped => {}
        }
    }

    if name.is_empty() {
        return QueryMatchResult::Skip;
    }

    QueryMatchResult::Chunk(MatchData { name, kind, node })
}

/// Process a single tree-sitter query match into a `QueryMatchResult`.
pub(crate) fn process_query_match<'tree, C: LanguageConfig>(
    m: &tree_sitter::QueryMatch<'_, 'tree>,
    source_bytes: &[u8],
    config: &C,
    root_node: tree_sitter::Node<'tree>,
) -> QueryMatchResult<'tree> {
    let classifications = classify_captures(m, source_bytes, config);
    assemble_match_result(classifications, root_node)
}

/// Check deduplication and return false if this match was already seen (operation: logic only).
pub(crate) fn check_dedup(
    needs_dedup: bool,
    name: &str,
    start_line: u32,
    seen: &mut HashSet<(String, u32)>,
) -> bool {
    if !needs_dedup {
        return true;
    }
    let key = (name.to_string(), start_line);
    if seen.contains(&key) {
        return false;
    }
    seen.insert(key);
    true
}

/// Gathered metadata for a chunk.
pub(crate) struct ChunkMetadata {
    pub parent: Option<String>,
    pub kind: ChunkKind,
    pub visibility: Option<String>,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub attributes: Option<String>,
    pub ui_ctx: Option<String>,
    pub should_skip: bool,
}

/// Gather metadata for a chunk via config trait methods (integration: calls only).
pub(crate) fn gather_chunk_metadata<C: LanguageConfig>(
    data: &MatchData<'_>,
    source_bytes: &[u8],
    content: &str,
    config: &C,
) -> ChunkMetadata {
    let parent = config.find_parent(data.node, source_bytes);
    let mut kind = data.kind.clone();
    let should_skip = config.should_skip_function(&kind, &parent);
    let mut visibility = config.extract_visibility(content);
    config.adjust_chunk_metadata(&mut kind, &data.name, &parent, &mut visibility);
    let signature = config.extract_signature(content, &kind);
    let doc_comment = config.collect_doc_comment(data.node, source_bytes);
    let attributes = config.collect_attributes(data.node, source_bytes);
    let ui_ctx = config.ui_ctx();

    ChunkMetadata {
        parent,
        kind,
        visibility,
        signature,
        doc_comment,
        attributes,
        ui_ctx,
        should_skip,
    }
}

/// Build a `Chunk` from match data, handling deduplication and skip logic.
pub(crate) fn build_chunk_from_match_data<C: LanguageConfig>(
    data: &MatchData<'_>,
    source_bytes: &[u8],
    file_id: i64,
    config: &C,
    seen: &mut HashSet<(String, u32)>,
) -> Option<Chunk> {
    let start = data.node.start_position();
    let start_line = start.row as u32 + 1;

    if !check_dedup(config.needs_deduplication(), &data.name, start_line, seen) {
        return None;
    }

    let content = data.node.utf8_text(source_bytes).unwrap_or("").to_string();
    let end = data.node.end_position();

    let meta = gather_chunk_metadata(data, source_bytes, &content, config);
    if meta.should_skip {
        return None;
    }

    Some(Chunk {
        start_line,
        end_line: end.row as u32 + 1,
        start_byte: data.node.start_byte() as u32,
        end_byte: data.node.end_byte() as u32,
        kind: meta.kind,
        ident: data.name.clone(),
        parent: meta.parent,
        signature: meta.signature,
        visibility: meta.visibility,
        ui_ctx: meta.ui_ctx,
        doc_comment: meta.doc_comment,
        attributes: meta.attributes,
        content,
        ..Chunk::stub(file_id)
    })
}

/// Build an imports chunk from collected import declaration nodes (operation: logic only).
pub(crate) fn build_import_chunk(
    import_decls: &[tree_sitter::Node<'_>],
    source_bytes: &[u8],
    file_id: i64,
) -> Option<Chunk> {
    if import_decls.is_empty() {
        return None;
    }

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

    Some(Chunk {
        start_line: start_line as u32 + 1,
        end_line: end_line as u32 + 1,
        start_byte: start_byte as u32,
        end_byte: end_byte as u32,
        kind: ChunkKind::Other("imports".into()),
        ident: "_imports".to_string(),
        content,
        ..Chunk::stub(file_id)
    })
}
