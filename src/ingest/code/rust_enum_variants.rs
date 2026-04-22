//! Enum-variant extraction helpers for the Rust parser (task #116).
//!
//! Enum variants are nested inside an `enum_item` AST node. The main
//! `chunk.scm` query captures the enum itself; this module walks into
//! the captured enum's `enum_variant_list` children and emits one
//! [`ChunkKind::EnumVariant`] chunk per variant, with
//! `parent = <enum_name>` for disambiguation.
//!
//! The list-walking skeleton is shared with impl-method extraction via
//! [`crate::ingest::code::rust_nested`].

use crate::domain::chunk::{Chunk, ChunkKind};
use crate::ingest::code::rust_nested::{extract_nested, rust_doc_and_attrs, NestedKind, NestedRaw};

pub(crate) fn extract_enum_variants(
    enum_node: tree_sitter::Node,
    source: &[u8],
    file_id: i64,
    enum_name: &str,
) -> Vec<Chunk> {
    let kind = NestedKind {
        list_kind: "enum_variant_list",
        item_kind: "enum_variant",
        name_kinds: &["type_identifier", "identifier"],
        build: build_variant,
    };
    extract_nested(enum_node, source, file_id, enum_name, &kind)
}

fn build_variant(
    raw: &NestedRaw,
    node: tree_sitter::Node,
    source: &[u8],
    enum_name: &str,
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
        kind: ChunkKind::EnumVariant,
        ident: raw.name.clone(),
        parent: Some(enum_name.to_string()),
        signature: None,
        visibility: None,
        ui_ctx: None,
        doc_comment,
        attributes,
        content: raw.content.clone(),
    }
}
