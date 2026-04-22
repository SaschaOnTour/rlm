//! Shared nested-chunk extraction for the Rust parser.
//!
//! Both impl methods and enum variants follow the same shape:
//!
//! 1. The parent (`impl_item` / `enum_item`) wraps a list node
//!    (`declaration_list` / `enum_variant_list`).
//! 2. The list contains item nodes (`function_item` / `enum_variant`).
//! 3. Each item contributes a [`Chunk`] with `parent = <outer name>`.
//!
//! This module parameterises that pattern. Sibling modules (impl methods,
//! enum variants) supply a [`NestedKind`] describing the AST node kinds
//! and per-item chunk assembly.

use crate::domain::chunk::Chunk;
use crate::ingest::code::base::{
    collect_prev_siblings, collect_prev_siblings_filtered_skip, SiblingCollectConfig,
};

/// Raw byte-range + name + content pulled from an item node, before the
/// language-specific chunk assembly step.
pub(crate) struct NestedRaw {
    pub name: String,
    pub content: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: u32,
    pub end_byte: u32,
}

/// Per-kind configuration: what list + item AST nodes to look for, and
/// how to assemble a `Chunk` from the raw data and the surrounding
/// doc-comment / attribute sidecars.
pub(crate) struct NestedKind<'a> {
    pub list_kind: &'a str,
    pub item_kind: &'a str,
    /// Field names (or raw kinds) that identify the item's name token.
    /// Used by `item_name` to find the identifier child.
    pub name_kinds: &'a [&'a str],
    /// Builder receives raw item data + the tree-sitter node (for doc /
    /// attribute collection) + the outer parent's name.
    pub build: fn(&NestedRaw, tree_sitter::Node, &[u8], &str, i64) -> Chunk,
}

/// Grouped inputs to [`extract_nested`] and the internal
/// `collect_from_list`. Keeps call-sites under the 5-parameter ceiling.
struct NestedCtx<'a> {
    source: &'a [u8],
    file_id: i64,
    parent_name: &'a str,
    kind: &'a NestedKind<'a>,
}

pub(crate) fn extract_nested(
    parent_node: tree_sitter::Node,
    source: &[u8],
    file_id: i64,
    parent_name: &str,
    kind: &NestedKind,
) -> Vec<Chunk> {
    let ctx = NestedCtx {
        source,
        file_id,
        parent_name,
        kind,
    };
    let mut out = Vec::new();
    for i in 0..parent_node.child_count() {
        let Some(child) = parent_node.child(i as u32) else {
            continue;
        };
        collect_from_list(child, &ctx, &mut out);
    }
    out
}

fn collect_from_list(list: tree_sitter::Node, ctx: &NestedCtx, out: &mut Vec<Chunk>) {
    if list.kind() != ctx.kind.list_kind {
        return;
    }
    for j in 0..list.child_count() {
        let Some(item) = list.child(j as u32) else {
            continue;
        };
        if item.kind() != ctx.kind.item_kind {
            continue;
        }
        let Some(raw) = item_raw(item, ctx.source, ctx.kind.name_kinds) else {
            continue;
        };
        out.push((ctx.kind.build)(
            &raw,
            item,
            ctx.source,
            ctx.parent_name,
            ctx.file_id,
        ));
    }
}

fn item_raw(item: tree_sitter::Node, source: &[u8], name_kinds: &[&str]) -> Option<NestedRaw> {
    let name = item_name(item, source, name_kinds);
    if name.is_empty() {
        return None;
    }
    let content = item.utf8_text(source).unwrap_or("").to_string();
    let start = item.start_position();
    let end = item.end_position();
    Some(NestedRaw {
        name,
        content,
        start_line: start.row as u32 + 1,
        end_line: end.row as u32 + 1,
        start_byte: item.start_byte() as u32,
        end_byte: item.end_byte() as u32,
    })
}

fn item_name(item: tree_sitter::Node, source: &[u8], name_kinds: &[&str]) -> String {
    for k in 0..item.child_count() {
        let Some(child) = item.child(k as u32) else {
            continue;
        };
        if name_kinds.contains(&child.kind()) {
            return child.utf8_text(source).unwrap_or("").to_string();
        }
    }
    String::new()
}

/// Convenience: collect the standard Rust `doc-comment` + `attribute`
/// sidecars from the siblings before `node`. Used by both impl methods
/// and enum variants; factored here to keep the builders tiny.
pub(crate) fn rust_doc_and_attrs(
    node: tree_sitter::Node,
    source: &[u8],
) -> (Option<String>, Option<String>) {
    let doc_config = SiblingCollectConfig::rust_doc_comments();
    let attr_config = SiblingCollectConfig::rust_attributes();
    (
        collect_prev_siblings(node, source, &doc_config),
        collect_prev_siblings_filtered_skip(node, source, &attr_config),
    )
}
