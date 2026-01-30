//! Type info operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting type information about symbols,
//! including prioritization of chunks from src/ over fixtures/tests.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

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

    // Get all files for prioritization
    let files = db.get_all_files()?;

    // Prioritize chunks: src/ > default > fixtures/tests
    let chunk = chunks
        .iter()
        .min_by_key(|c| {
            let file = files.iter().find(|f| f.id == c.file_id);
            match file {
                Some(f) => {
                    if f.path.starts_with("src/") {
                        0 // Highest priority for src/
                    } else if f.path.contains("fixtures") || f.path.contains("test") {
                        2 // Lowest priority for fixtures/tests
                    } else {
                        1 // Medium priority for everything else
                    }
                }
                None => 3, // Unknown files get lowest priority
            }
        })
        .unwrap(); // Safe: we already checked chunks is not empty

    let file_path = files
        .iter()
        .find(|f| f.id == chunk.file_id)
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

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn get_type_info_basic() {
        let db = test_db();

        // Insert a file and chunk
        let file = FileRecord::new("src/lib.rs".into(), "hash".into(), "rust".into(), 100);
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
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

        // Insert file in fixtures
        let fixture_file =
            FileRecord::new("fixtures/test.rs".into(), "h1".into(), "rust".into(), 100);
        let fixture_id = db.upsert_file(&fixture_file).unwrap();

        // Insert file in src
        let src_file = FileRecord::new("src/lib.rs".into(), "h2".into(), "rust".into(), 100);
        let src_id = db.upsert_file(&src_file).unwrap();

        // Same symbol in both files
        let fixture_chunk = Chunk {
            id: 0,
            file_id: fixture_id,
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: 30,
            kind: ChunkKind::Function,
            ident: "foo".into(),
            parent: None,
            signature: Some("fn foo() [fixture]".into()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn foo() { fixture }".into(),
        };
        db.insert_chunk(&fixture_chunk).unwrap();

        let src_chunk = Chunk {
            id: 0,
            file_id: src_id,
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: 30,
            kind: ChunkKind::Function,
            ident: "foo".into(),
            parent: None,
            signature: Some("fn foo() [src]".into()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn foo() { src }".into(),
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
