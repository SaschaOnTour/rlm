//! Scope operations shared between CLI and MCP.
//!
//! Provides consistent behavior for determining what symbols are visible at a location.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

/// Result of getting scope information.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeResult {
    /// The file path.
    #[serde(rename = "f")]
    pub file: String,
    /// The line number.
    #[serde(rename = "l")]
    pub line: u32,
    /// Symbols that contain this line (scopes we're inside of).
    #[serde(rename = "in")]
    pub containing: Vec<String>,
    /// Symbols visible at this location.
    pub visible: Vec<String>,
}

/// Get what symbols are visible at a specific line in a file.
pub fn get_scope(db: &Database, path: &str, line: u32) -> Result<ScopeResult> {
    let file = db
        .get_file_by_path(path)?
        .ok_or_else(|| crate::error::RlmError::Other(format!("file not found: {path}")))?;

    let chunks = db.get_chunks_for_file(file.id)?;

    // Find chunks that contain this line
    let containing: Vec<String> = chunks
        .iter()
        .filter(|c| line >= c.start_line && line <= c.end_line)
        .map(|c| c.ident.clone())
        .collect();

    // Find visible symbols: all items defined before this line
    let visible: Vec<String> = chunks
        .iter()
        .filter(|c| c.start_line <= line)
        .map(|c| format!("{}:{}", c.kind.as_str(), c.ident))
        .collect();

    Ok(ScopeResult {
        file: path.to_string(),
        line,
        containing,
        visible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    const TEST_FILE_BYTES: u64 = 100;
    const TEST_START_LINE: u32 = 1;
    const TEST_END_LINE: u32 = 5;
    const TEST_START_BYTE: u32 = 0;
    const TEST_END_BYTE: u32 = 50;
    const BAR_START_LINE: u32 = 7;
    const BAR_END_LINE: u32 = 15;
    const BAR_START_BYTE: u32 = 51;
    const BAR_END_BYTE: u32 = 150;
    const QUERY_LINE: u32 = 10;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn get_scope_basic() {
        let db = test_db();

        let file = FileRecord::new(
            "src/lib.rs".into(),
            "hash".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        let file_id = db.upsert_file(&file).unwrap();

        // First function
        let chunk1 = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE,
            kind: ChunkKind::Function,
            ident: "foo".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn foo() {}".into(),
        };
        db.insert_chunk(&chunk1).unwrap();

        // Second function
        let chunk2 = Chunk {
            id: 0,
            file_id,
            start_line: BAR_START_LINE,
            end_line: BAR_END_LINE,
            start_byte: BAR_START_BYTE,
            end_byte: BAR_END_BYTE,
            kind: ChunkKind::Function,
            ident: "bar".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn bar() {}".into(),
        };
        db.insert_chunk(&chunk2).unwrap();

        // Query at line QUERY_LINE (inside bar)
        let result = get_scope(&db, "src/lib.rs", QUERY_LINE).unwrap();
        assert_eq!(result.file, "src/lib.rs");
        assert_eq!(result.line, QUERY_LINE);
        assert_eq!(result.containing, vec!["bar"]);
        // Both foo and bar are visible (defined before line 10)
        assert!(result.visible.contains(&"fn:foo".to_string()));
        assert!(result.visible.contains(&"fn:bar".to_string()));
    }

    #[test]
    fn get_scope_file_not_found() {
        let db = test_db();
        let result = get_scope(&db, "nonexistent.rs", 1);
        assert!(result.is_err());
    }
}
