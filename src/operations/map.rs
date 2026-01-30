//! Map building shared between CLI and MCP.

use std::collections::HashMap;

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

/// A single entry in the project map.
#[derive(Debug, Clone, Serialize)]
pub struct MapEntry {
    /// File path.
    #[serde(rename = "f")]
    pub file: String,
    /// Language identifier.
    #[serde(rename = "l")]
    pub lang: String,
    /// Number of lines in the file.
    #[serde(rename = "lc")]
    pub line_count: u32,
    /// Public symbols in format "kind:name".
    #[serde(rename = "s")]
    pub symbols: Vec<String>,
    /// Description of file contents (e.g., "3 fn, 2 struct").
    #[serde(rename = "d")]
    pub description: String,
}

/// Build a project map showing file overview.
///
/// For each file (optionally filtered by path prefix), returns:
/// - Language
/// - Line count
/// - Public symbols
/// - Description of contained items
pub fn build_map(db: &Database, path_filter: Option<&str>) -> Result<Vec<MapEntry>> {
    let files = db.get_all_files()?;

    let mut entries = Vec::new();
    for file in &files {
        if let Some(filter) = path_filter {
            if !file.path.starts_with(filter) {
                continue;
            }
        }

        let chunks = db.get_chunks_for_file(file.id)?;
        let max_line = chunks.iter().map(|c| c.end_line).max().unwrap_or(0);
        let pub_symbols: Vec<String> = chunks
            .iter()
            .filter(|c| {
                c.visibility
                    .as_ref()
                    .is_some_and(|v| v == "pub" || v == "public")
            })
            .map(|c| format!("{}:{}", c.kind.as_str(), c.ident))
            .collect();

        let kind_counts: HashMap<&str, usize> = chunks.iter().fold(HashMap::new(), |mut m, c| {
            *m.entry(c.kind.as_str()).or_insert(0) += 1;
            m
        });

        let desc_parts: Vec<String> = kind_counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect();

        entries.push(MapEntry {
            file: file.path.clone(),
            lang: file.lang.clone(),
            line_count: max_line,
            symbols: pub_symbols,
            description: desc_parts.join(", "),
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    fn setup_test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn test_map_empty_db() {
        let db = setup_test_db();
        let result = build_map(&db, None).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_map_basic() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/lib.rs".to_string(),
            "abc123".to_string(),
            "rust".to_string(),
            500,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let pub_fn = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 10,
            start_byte: 0,
            end_byte: 100,
            kind: ChunkKind::Function,
            ident: "process".to_string(),
            parent: None,
            signature: Some("fn process()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "pub fn process() { }".to_string(),
        };
        db.insert_chunk(&pub_fn).unwrap();

        let priv_fn = Chunk {
            id: 0,
            file_id,
            start_line: 15,
            end_line: 20,
            start_byte: 150,
            end_byte: 200,
            kind: ChunkKind::Function,
            ident: "helper".to_string(),
            parent: None,
            signature: Some("fn helper()".to_string()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn helper() { }".to_string(),
        };
        db.insert_chunk(&priv_fn).unwrap();

        let pub_struct = Chunk {
            id: 0,
            file_id,
            start_line: 25,
            end_line: 30,
            start_byte: 250,
            end_byte: 300,
            kind: ChunkKind::Struct,
            ident: "Config".to_string(),
            parent: None,
            signature: Some("struct Config".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "pub struct Config { }".to_string(),
        };
        db.insert_chunk(&pub_struct).unwrap();

        let result = build_map(&db, None).unwrap();

        assert_eq!(result.len(), 1);
        let entry = &result[0];
        assert_eq!(entry.file, "src/lib.rs");
        assert_eq!(entry.lang, "rust");
        assert_eq!(entry.line_count, 30);
        assert_eq!(entry.symbols.len(), 2);
        assert!(entry.symbols.contains(&"fn:process".to_string()));
        assert!(entry.symbols.contains(&"struct:Config".to_string()));
        assert!(entry.description.contains("fn"));
        assert!(entry.description.contains("struct"));
    }

    #[test]
    fn test_map_with_path_filter() {
        let db = setup_test_db();

        let file1 = FileRecord::new(
            "src/lib.rs".to_string(),
            "aaa".to_string(),
            "rust".to_string(),
            100,
        );
        let file1_id = db.upsert_file(&file1).unwrap();

        let file2 = FileRecord::new(
            "tests/test.rs".to_string(),
            "bbb".to_string(),
            "rust".to_string(),
            100,
        );
        let file2_id = db.upsert_file(&file2).unwrap();

        let chunk1 = Chunk {
            id: 0,
            file_id: file1_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
            kind: ChunkKind::Function,
            ident: "main".to_string(),
            parent: None,
            signature: Some("fn main()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() { }".to_string(),
        };
        db.insert_chunk(&chunk1).unwrap();

        let chunk2 = Chunk {
            id: 0,
            file_id: file2_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
            kind: ChunkKind::Function,
            ident: "test_fn".to_string(),
            parent: None,
            signature: Some("fn test_fn()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn test_fn() { }".to_string(),
        };
        db.insert_chunk(&chunk2).unwrap();

        let result = build_map(&db, Some("src/")).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file, "src/lib.rs");
    }

    #[test]
    fn test_map_multiple_files() {
        let db = setup_test_db();

        let file1 = FileRecord::new(
            "src/a.rs".to_string(),
            "aaa".to_string(),
            "rust".to_string(),
            100,
        );
        db.upsert_file(&file1).unwrap();

        let file2 = FileRecord::new(
            "src/b.rs".to_string(),
            "bbb".to_string(),
            "rust".to_string(),
            100,
        );
        db.upsert_file(&file2).unwrap();

        let result = build_map(&db, None).unwrap();

        assert_eq!(result.len(), 2);
        let files: Vec<&str> = result.iter().map(|e| e.file.as_str()).collect();
        assert!(files.contains(&"src/a.rs"));
        assert!(files.contains(&"src/b.rs"));
    }

    #[test]
    fn test_map_public_visibility() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/lib.rs".to_string(),
            "xyz".to_string(),
            "java".to_string(),
            200,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let pub_method = Chunk {
            id: 0,
            file_id,
            start_line: 5,
            end_line: 10,
            start_byte: 50,
            end_byte: 150,
            kind: ChunkKind::Method,
            ident: "process".to_string(),
            parent: Some("MyClass".to_string()),
            signature: Some("public void process()".to_string()),
            visibility: Some("public".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "public void process() { }".to_string(),
        };
        db.insert_chunk(&pub_method).unwrap();

        let priv_method = Chunk {
            id: 0,
            file_id,
            start_line: 15,
            end_line: 20,
            start_byte: 200,
            end_byte: 300,
            kind: ChunkKind::Method,
            ident: "helper".to_string(),
            parent: Some("MyClass".to_string()),
            signature: Some("private void helper()".to_string()),
            visibility: Some("private".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "private void helper() { }".to_string(),
        };
        db.insert_chunk(&priv_method).unwrap();

        let result = build_map(&db, None).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].symbols.len(), 1);
        assert!(result[0].symbols.contains(&"method:process".to_string()));
    }
}
