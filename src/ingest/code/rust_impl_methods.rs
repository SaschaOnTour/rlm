//! Impl-block method extraction helpers for the Rust parser.
//!
//! Extracted from `rust.rs` for SRP compliance. The list-walking skeleton
//! is shared with enum-variant extraction via [`super::rust_nested`].

use crate::domain::chunk::{Chunk, ChunkKind};
use crate::ingest::code::rust_nested::{extract_nested, rust_doc_and_attrs, NestedKind, NestedRaw};

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

pub(crate) fn extract_impl_methods(
    impl_node: tree_sitter::Node,
    source: &[u8],
    file_id: i64,
    impl_name: &str,
) -> Vec<Chunk> {
    let kind = NestedKind {
        list_kind: "declaration_list",
        item_kind: "function_item",
        name_kinds: &["identifier"],
        build: build_method,
    };
    extract_nested(impl_node, source, file_id, impl_name, &kind)
}

fn build_method(
    raw: &NestedRaw,
    node: tree_sitter::Node,
    source: &[u8],
    impl_name: &str,
    file_id: i64,
) -> Chunk {
    let (doc_comment, attributes) = rust_doc_and_attrs(node, source);
    Chunk {
        id: 0,
        file_id,
        start_line: raw.start_line,
        end_line: raw.end_line,
        start_byte: raw.start_byte,
        end_byte: raw.end_byte,
        kind: ChunkKind::Method,
        ident: raw.name.clone(),
        parent: Some(impl_name.to_string()),
        signature: extract_fn_signature(&raw.content),
        visibility: extract_rust_visibility(&raw.content),
        ui_ctx: None,
        doc_comment,
        attributes,
        content: raw.content.clone(),
    }
}
