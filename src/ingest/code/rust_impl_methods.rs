//! Impl-block method extraction helpers for the Rust parser.
//!
//! Extracted from `rust.rs` for SRP compliance. Contains the tree-walking
//! and chunk-building logic that extracts individual methods from `impl` blocks.

use crate::ingest::code::base::{
    collect_prev_siblings, collect_prev_siblings_filtered_skip, SiblingCollectConfig,
};
use crate::models::chunk::{Chunk, ChunkKind};

use super::rust::{extract_fn_signature, extract_rust_visibility};

/// Find a tree-sitter node at the given byte range.
pub(crate) fn find_node_at_byte_range(
    root: tree_sitter::Node,
    start_byte: usize,
    end_byte: usize,
) -> Option<tree_sitter::Node> {
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.start_byte() == start_byte && node.end_byte() == end_byte {
            return Some(node);
        }
        if !cursor.goto_first_child() {
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return None;
                }
            }
        }
    }
}

/// Extract all methods from an impl block (integration: calls only, no logic).
///
/// Walks the impl node's children to find `declaration_list` nodes,
/// then delegates each function item to `build_method_chunk`.
pub(crate) fn extract_impl_methods(
    impl_node: tree_sitter::Node,
    source: &[u8],
    file_id: i64,
    impl_name: &str,
) -> Vec<Chunk> {
    let mut methods = Vec::new();

    for i in 0..impl_node.child_count() {
        let child = match impl_node.child(i as u32) {
            Some(c) => c,
            None => continue,
        };
        collect_methods_from_decl_list(child, source, file_id, impl_name, &mut methods);
    }

    methods
}

/// Raw data extracted from a function_item node (before calling helpers).
struct RawMethodData {
    fn_name: String,
    content: String,
    start_row: u32,
    end_row: u32,
    start_byte: u32,
    end_byte: u32,
}

/// Extract raw method data from a declaration_list node (operation: logic only).
fn extract_raw_methods<'a>(decl_list: tree_sitter::Node<'a>, source: &'a [u8]) -> Vec<(RawMethodData, tree_sitter::Node<'a>)> {
    if decl_list.kind() != "declaration_list" {
        return Vec::new();
    }

    let mut results = Vec::new();

    for j in 0..decl_list.child_count() {
        let item = match decl_list.child(j as u32) {
            Some(c) => c,
            None => continue,
        };

        if item.kind() != "function_item" {
            continue;
        }

        let fn_name = match (0..item.child_count())
            .filter_map(|k| item.child(k as u32))
            .find(|n| n.kind() == "identifier")
        {
            Some(n) => n.utf8_text(source).unwrap_or("").to_string(),
            None => String::new(),
        };

        if fn_name.is_empty() {
            continue;
        }

        let content = item.utf8_text(source).unwrap_or("").to_string();
        let start = item.start_position();
        let end = item.end_position();

        results.push((
            RawMethodData {
                fn_name,
                content,
                start_row: start.row as u32,
                end_row: end.row as u32,
                start_byte: item.start_byte() as u32,
                end_byte: item.end_byte() as u32,
            },
            item,
        ));
    }

    results
}

/// Build method chunks from raw method data (integration: calls only).
fn collect_methods_from_decl_list(
    decl_list: tree_sitter::Node,
    source: &[u8],
    file_id: i64,
    impl_name: &str,
    methods: &mut Vec<Chunk>,
) {
    let raw_methods = extract_raw_methods(decl_list, source);

    for (data, node) in raw_methods {
        let doc_comment = collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["line_comment"],
                skip_kinds: &["attribute_item"],
                prefixes: &["///", "//!"],
                multi: true,
            },
        );
        let attributes = collect_prev_siblings_filtered_skip(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["attribute_item"],
                skip_kinds: &["line_comment"],
                prefixes: &["///", "//!"],
                multi: true,
            },
        );

        methods.push(Chunk {
            id: 0,
            file_id,
            start_line: data.start_row + 1,
            end_line: data.end_row + 1,
            start_byte: data.start_byte,
            end_byte: data.end_byte,
            kind: ChunkKind::Method,
            ident: data.fn_name,
            parent: Some(impl_name.to_string()),
            signature: extract_fn_signature(&data.content),
            visibility: extract_rust_visibility(&data.content),
            ui_ctx: None,
            doc_comment,
            attributes,
            content: data.content,
        });
    }
}
