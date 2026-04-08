use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;
use crate::models::chunk::Chunk;

/// A node in the file tree.
#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    #[serde(rename = "n")]
    pub name: String,
    #[serde(rename = "p")]
    pub path: String,
    #[serde(rename = "dir")]
    pub is_dir: bool,
    #[serde(rename = "s")]
    pub symbols: Vec<SymbolInfo>,
    #[serde(rename = "ch")]
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
    #[serde(rename = "k")]
    pub kind: String,
    #[serde(rename = "n")]
    pub name: String,
    #[serde(rename = "l")]
    pub line: u32,
}

/// Build a tree view of the indexed codebase with symbol annotations.
/// When `path_filter` is set, only files whose path starts with the prefix are included.
// qual:allow(iosp) reason: "minimal orchestration: fetch data then build tree"
pub fn build_tree(db: &Database, path_filter: Option<&str>) -> Result<Vec<TreeNode>> {
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

    Ok(root_children.into_values().collect())
}

// qual:recursive
// qual:allow(iosp) reason: "recursive tree construction inherently mixes branching with delegation"
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
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    /// File size in bytes for the test file record.
    const TEST_FILE_SIZE: u64 = 100;
    /// End line of the test chunk.
    const CHUNK_END_LINE: u32 = 3;
    /// End byte offset of the test chunk content "fn main() {}".
    const CHUNK_END_BYTE: u32 = 30;

    #[test]
    fn build_tree_from_db() {
        let db = Database::open_in_memory().unwrap();
        let f1 = FileRecord::new(
            "src/main.rs".into(),
            "h1".into(),
            "rust".into(),
            TEST_FILE_SIZE,
        );
        let fid = db.upsert_file(&f1).unwrap();
        let c = Chunk {
            id: 0,
            file_id: fid,
            start_line: 1,
            end_line: CHUNK_END_LINE,
            start_byte: 0,
            end_byte: CHUNK_END_BYTE,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: Some("fn main()".into()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() {}".into(),
        };
        db.insert_chunk(&c).unwrap();

        let tree = build_tree(&db, None).unwrap();
        assert!(!tree.is_empty());
        let formatted = format_tree(&tree, 0);
        assert!(formatted.contains("src/"));
        assert!(formatted.contains("main.rs"));
        assert!(formatted.contains("fn:main"));

        // Verify structured JSON serialization
        let json = serde_json::to_string(&tree).unwrap();
        assert!(
            json.contains("\"n\":"),
            "should have short key 'n' for name"
        );
        assert!(
            json.contains("\"ch\":"),
            "should have short key 'ch' for children"
        );
        assert!(
            json.contains("\"s\":"),
            "should have short key 's' for symbols"
        );
        assert!(json.contains("\"dir\":"), "should have 'dir' key");
        assert!(
            json.contains("\"k\":"),
            "should have short key 'k' for symbol kind"
        );
        assert!(
            json.contains("\"l\":"),
            "should have short key 'l' for line"
        );
    }

    #[test]
    fn format_tree_empty() {
        let formatted = format_tree(&[], 0);
        assert!(formatted.is_empty());
    }
}
