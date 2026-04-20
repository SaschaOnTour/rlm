use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Wrapped tree result with token estimate.
#[derive(Debug, Clone, Serialize)]
pub struct TreeResult {
    /// Tree nodes.
    pub results: Vec<TreeNode>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// A node in the file tree.
#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub symbols: Vec<SymbolInfo>,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    /// Create a new tree node.
    fn new(name: String, path: String, is_dir: bool, symbols: Vec<SymbolInfo>) -> Self {
        Self {
            name,
            path,
            is_dir,
            symbols,
            children: Vec::new(),
        }
    }
}

/// Symbol summary for tree display.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolInfo {
    pub kind: String,
    pub name: String,
    pub line: u32,
}

/// Build a tree view of the indexed codebase with symbol annotations.
/// When `path_filter` is set, only files whose path starts with the prefix are included.
pub fn build_tree(db: &Database, path_filter: Option<&str>) -> Result<TreeResult> {
    let mut files = db.get_all_files()?;
    if let Some(prefix) = path_filter {
        files.retain(|f| f.path.starts_with(prefix));
    }
    let all_chunks = db.get_all_chunks()?;

    // Group chunks by file_id
    let mut chunks_by_file: BTreeMap<i64, Vec<&Chunk>> = BTreeMap::new();
    for chunk in &all_chunks {
        chunks_by_file.entry(chunk.file_id).or_default().push(chunk);
    }

    // Build directory tree
    let mut root_children: BTreeMap<String, TreeNode> = BTreeMap::new();

    for file in &files {
        let parts: Vec<&str> = file.path.split('/').collect();
        let symbols: Vec<SymbolInfo> = chunks_by_file
            .get(&file.id)
            .map(|chunks| {
                chunks
                    .iter()
                    .map(|c| SymbolInfo {
                        kind: c.kind.as_str().to_string(),
                        name: c.ident.clone(),
                        line: c.start_line,
                    })
                    .collect()
            })
            .unwrap_or_default();

        insert_into_tree(&mut root_children, &parts, &file.path, symbols);
    }

    let nodes: Vec<TreeNode> = root_children.into_values().collect();
    let mut result = TreeResult {
        results: nodes,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

// qual:recursive
fn insert_into_tree(
    children: &mut BTreeMap<String, TreeNode>,
    parts: &[&str],
    full_path: &str,
    symbols: Vec<SymbolInfo>,
) {
    if parts.is_empty() {
        return;
    }

    let name = parts[0].to_string();

    if parts.len() == 1 {
        // Leaf: file node
        children.insert(
            name.clone(),
            TreeNode::new(name, full_path.to_string(), false, symbols),
        );
    } else {
        // Directory node
        let dir = children
            .entry(name.clone())
            .or_insert_with(|| TreeNode::new(name.clone(), String::new(), true, Vec::new()));

        let mut child_map: BTreeMap<String, TreeNode> = BTreeMap::new();
        for child in dir.children.drain(..) {
            child_map.insert(child.name.clone(), child);
        }
        insert_into_tree(&mut child_map, &parts[1..], full_path, symbols);
        dir.children = child_map.into_values().collect();
    }
}

/// Format a tree as a string with indentation and symbol annotations.
// qual:recursive
#[must_use]
pub fn format_tree(nodes: &[TreeNode], indent: usize) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for node in nodes {
        let prefix = "  ".repeat(indent);
        if node.is_dir {
            let _ = writeln!(out, "{prefix}{}/", node.name);
            out.push_str(&format_tree(&node.children, indent + 1));
        } else {
            let _ = write!(out, "{prefix}{}", node.name);
            if !node.symbols.is_empty() {
                let sym_list: Vec<String> = node
                    .symbols
                    .iter()
                    .map(|s| format!("{}:{}", s.kind, s.name))
                    .collect();
                let _ = write!(out, "  [{}]", sym_list.join(", "));
            }
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
