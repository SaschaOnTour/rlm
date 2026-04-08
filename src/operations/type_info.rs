//! Type info operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting type information about symbols,
//! including prioritization of chunks from src/ over fixtures/tests.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

/// Priority value assigned to chunks whose file record is unknown,
/// ensuring they sort below src/ (0), default (1), and fixtures/tests (2).
const UNKNOWN_FILE_PRIORITY: i32 = 3;

/// Result of getting type information for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct TypeInfoResult {
    /// The symbol name.
    #[serde(rename = "s")]
    pub symbol: String,
    /// The kind of the symbol (fn, struct, class, etc.).
    #[serde(rename = "k")]
    pub kind: String,
    /// The signature if available.
    #[serde(rename = "sig")]
    pub signature: Option<String>,
    /// The full content of the symbol.
    #[serde(rename = "c")]
    pub content: String,
    /// The file path where the symbol is defined.
    #[serde(rename = "f")]
    pub file: String,
}

/// Get type information for a symbol.
///
/// Prioritizes chunks from:
/// 1. `src/` directory (highest priority)
/// 2. Default directories
/// 3. `fixtures/` or `test` directories (lowest priority)
///
/// This ensures consistent results when a symbol exists in multiple locations
/// (e.g., both in source and test fixtures).
pub fn get_type_info(db: &Database, symbol: &str) -> Result<TypeInfoResult> {
    let chunks = db.get_chunks_by_ident(symbol)?;

    if chunks.is_empty() {
        return Err(crate::error::RlmError::Other(format!(
            "symbol not found: {symbol}"
        )));
    }

    // Build file lookup for O(1) access instead of O(chunks * files)
    let files = db.get_all_files()?;
    let file_map: std::collections::HashMap<i64, &crate::models::file::FileRecord> =
        files.iter().map(|f| (f.id, f)).collect();

    // Prioritize chunks: src/ > default > fixtures/tests
    let chunk = chunks
        .iter()
        .min_by_key(|c| match file_map.get(&c.file_id) {
            Some(f) => {
                if f.path.starts_with("src/") {
                    0 // Highest priority for src/
                } else if f.path.contains("fixtures") || f.path.contains("test") {
                    2 // Lowest priority for fixtures/tests
                } else {
                    1 // Medium priority for everything else
                }
            }
            None => UNKNOWN_FILE_PRIORITY, // Unknown files get lowest priority
        })
        .ok_or_else(|| crate::error::RlmError::Other(format!("symbol not found: {symbol}")))?;

    let file_path = file_map
        .get(&chunk.file_id)
        .map(|f| f.path.clone())
        .unwrap_or_default();

    Ok(TypeInfoResult {
        symbol: symbol.to_string(),
        kind: chunk.kind.as_str().to_string(),
        signature: chunk.signature.clone(),
        content: chunk.content.clone(),
        file: file_path,
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
    const TEST_END_LINE_SHORT: u32 = 3;
    const TEST_START_BYTE: u32 = 0;
    const TEST_END_BYTE: u32 = 50;
    const TEST_END_BYTE_SMALL: u32 = 30;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn get_type_info_basic() {
        let db = test_db();

        // Insert a file and chunk
        let file = FileRecord::new(
            "src/lib.rs".into(),
            "hash".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE,
            kind: ChunkKind::Struct,
            ident: "MyStruct".into(),
            parent: None,
            signature: Some("struct MyStruct".into()),
            visibility: Some("pub".into()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "pub struct MyStruct {\n    field: i32,\n}".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        let result = get_type_info(&db, "MyStruct").unwrap();
        assert_eq!(result.symbol, "MyStruct");
        assert_eq!(result.kind, "struct");
        assert_eq!(result.signature, Some("struct MyStruct".into()));
        assert_eq!(result.file, "src/lib.rs");
    }

    #[test]
    fn get_type_info_prioritizes_src() {
        let db = test_db();

        let fixture_file = FileRecord::new(
            "fixtures/test.rs".into(),
            "h1".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        let fixture_id = db.upsert_file(&fixture_file).unwrap();

        let src_file = FileRecord::new(
            "src/lib.rs".into(),
            "h2".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        let src_id = db.upsert_file(&src_file).unwrap();

        // Same symbol in both files
        let fixture_chunk = Chunk {
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE_SHORT,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_SMALL,
            kind: ChunkKind::Function,
            ident: "foo".into(),
            signature: Some("fn foo() [fixture]".into()),
            content: "fn foo() { fixture }".into(),
            ..Chunk::stub(fixture_id)
        };
        db.insert_chunk(&fixture_chunk).unwrap();

        let src_chunk = Chunk {
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE_SHORT,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_SMALL,
            kind: ChunkKind::Function,
            ident: "foo".into(),
            signature: Some("fn foo() [src]".into()),
            content: "fn foo() { src }".into(),
            ..Chunk::stub(src_id)
        };
        db.insert_chunk(&src_chunk).unwrap();

        let result = get_type_info(&db, "foo").unwrap();
        // Should prioritize src/ over fixtures/
        assert_eq!(result.file, "src/lib.rs");
        assert_eq!(result.signature, Some("fn foo() [src]".into()));
    }

    #[test]
    fn get_type_info_symbol_not_found() {
        let db = test_db();
        let result = get_type_info(&db, "NonExistent");
        assert!(result.is_err());
    }
}
