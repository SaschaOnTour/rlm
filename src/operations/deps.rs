//! Dependencies operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting file dependencies using the
//! optimized `get_refs_for_file()` query instead of iterating over chunks.

use std::collections::HashSet;

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;
use crate::models::chunk::RefKind;

/// Result of getting dependencies for a file.
#[derive(Debug, Clone, Serialize)]
pub struct DepsResult {
    /// The file path.
    pub file: String,
    /// The list of imports/dependencies.
    pub imports: Vec<String>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Get all imports/dependencies for a file.
///
/// Uses the optimized `get_refs_for_file()` query which joins refs through chunks,
/// rather than iterating over each chunk individually.
pub fn get_deps(db: &Database, path: &str) -> Result<DepsResult> {
    let file = db
        .get_file_by_path(path)?
        .ok_or_else(|| crate::error::RlmError::FileNotFound {
            path: path.to_string(),
        })?;

    // Use the optimized file-level refs query
    let refs = db.get_refs_for_file(file.id)?;

    // Collect unique imports
    let mut imports = HashSet::new();
    for r in refs {
        if r.ref_kind == RefKind::Import {
            imports.insert(r.target_ident);
        }
    }

    // Sort for consistent output
    let mut import_list: Vec<String> = imports.into_iter().collect();
    import_list.sort();

    let mut result = DepsResult {
        file: path.to_string(),
        imports: import_list,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind, Reference};
    use crate::models::file::FileRecord;

    const TEST_FILE_BYTES: u64 = 100;
    const TEST_START_LINE: u32 = 1;
    const TEST_END_LINE: u32 = 10;
    const TEST_END_LINE_SHORT: u32 = 5;
    const TEST_START_BYTE: u32 = 0;
    const TEST_END_BYTE: u32 = 100;
    const TEST_END_BYTE_SMALL: u32 = 50;
    const TEST_END_BYTE_TINY: u32 = 10;
    const TEST_IMPORT_COL: u32 = 4;
    const TEST_CALL_COL: u32 = 12;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn get_deps_basic() {
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
            kind: ChunkKind::Module,
            ident: "lib".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "use std::io;\nuse crate::config;".into(),
        };
        let chunk_id = db.insert_chunk(&chunk).unwrap();

        // Insert import references
        let import1 = Reference {
            id: 0,
            chunk_id,
            target_ident: "std::io".into(),
            ref_kind: RefKind::Import,
            line: 1,
            col: TEST_IMPORT_COL,
        };
        db.insert_ref(&import1).unwrap();

        let import2 = Reference {
            id: 0,
            chunk_id,
            target_ident: "crate::config".into(),
            ref_kind: RefKind::Import,
            line: 2,
            col: TEST_IMPORT_COL,
        };
        db.insert_ref(&import2).unwrap();

        let result = get_deps(&db, "src/lib.rs").unwrap();
        assert_eq!(result.file, "src/lib.rs");
        assert_eq!(result.imports.len(), 2);
        assert!(result.imports.contains(&"std::io".to_string()));
        assert!(result.imports.contains(&"crate::config".to_string()));
    }

    #[test]
    fn get_deps_excludes_calls() {
        let db = test_db();

        let file = FileRecord::new(
            "src/main.rs".into(),
            "hash".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE_SHORT,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_SMALL,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() { foo(); }".into(),
        };
        let chunk_id = db.insert_chunk(&chunk).unwrap();

        // Insert a call reference (should be excluded from deps)
        let call_ref = Reference {
            id: 0,
            chunk_id,
            target_ident: "foo".into(),
            ref_kind: RefKind::Call,
            line: 1,
            col: TEST_CALL_COL,
        };
        db.insert_ref(&call_ref).unwrap();

        // Insert an import reference
        let import_ref = Reference {
            id: 0,
            chunk_id,
            target_ident: "std::io".into(),
            ref_kind: RefKind::Import,
            line: 1,
            col: TEST_IMPORT_COL,
        };
        db.insert_ref(&import_ref).unwrap();

        let result = get_deps(&db, "src/main.rs").unwrap();
        // Should only include imports, not calls
        assert_eq!(result.imports.len(), 1);
        assert!(result.imports.contains(&"std::io".to_string()));
        assert!(!result.imports.contains(&"foo".to_string()));
    }

    #[test]
    fn get_deps_file_not_found() {
        let db = test_db();
        let result = get_deps(&db, "nonexistent.rs");
        assert!(result.is_err());
    }

    #[test]
    fn get_deps_empty() {
        let db = test_db();

        let file = FileRecord::new(
            "src/empty.rs".into(),
            "hash".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_START_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_TINY,
            kind: ChunkKind::Function,
            ident: "f".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn f() {}".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        let result = get_deps(&db, "src/empty.rs").unwrap();
        assert!(result.imports.is_empty());
    }
}
