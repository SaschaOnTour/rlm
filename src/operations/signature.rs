//! Signature operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting symbol signatures and call site counts.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Result of getting a symbol's signature.
#[derive(Debug, Clone, Serialize)]
pub struct SignatureResult {
    /// The symbol name.
    pub symbol: String,
    /// The signatures (may have multiple if symbol is defined in multiple places).
    pub signatures: Vec<String>,
    /// The count of all call sites.
    pub ref_count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Get the signature of a symbol plus the count of all call sites.
pub fn get_signature(db: &Database, symbol: &str) -> Result<SignatureResult> {
    let chunks = db.get_chunks_by_ident(symbol)?;
    let refs = db.get_refs_to(symbol)?;

    let sigs: Vec<String> = chunks.iter().filter_map(|c| c.signature.clone()).collect();

    let mut result = SignatureResult {
        symbol: symbol.to_string(),
        signatures: sigs,
        ref_count: refs.len(),
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;

    const TEST_FILE_BYTES: u64 = 100;
    const TEST_START_LINE: u32 = 1;
    const TEST_END_LINE: u32 = 5;
    const TEST_END_LINE_SHORT: u32 = 3;
    const TEST_START_BYTE: u32 = 0;
    const TEST_END_BYTE: u32 = 50;
    const TEST_END_BYTE_SMALL: u32 = 30;
    const TEST_REF_LINE: u32 = 10;
    const TEST_REF_COL: u32 = 4;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn get_signature_basic() {
        let db = test_db();

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
            kind: ChunkKind::Function,
            ident: "foo".into(),
            parent: None,
            signature: Some("fn foo(x: i32) -> String".into()),
            visibility: Some("pub".into()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "pub fn foo(x: i32) -> String { }".into(),
        };
        let chunk_id = db.insert_chunk(&chunk).unwrap();

        // Add some refs
        let reference = Reference {
            id: 0,
            chunk_id,
            target_ident: "foo".into(),
            ref_kind: RefKind::Call,
            line: TEST_REF_LINE,
            col: TEST_REF_COL,
        };
        db.insert_ref(&reference).unwrap();

        let result = get_signature(&db, "foo").unwrap();
        assert_eq!(result.symbol, "foo");
        assert_eq!(result.signatures, vec!["fn foo(x: i32) -> String"]);
        assert_eq!(result.ref_count, 1);
    }

    #[test]
    fn get_signature_no_signature() {
        let db = test_db();

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
            end_line: TEST_END_LINE_SHORT,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_SMALL,
            kind: ChunkKind::Module,
            ident: "mymod".into(),
            parent: None,
            signature: None, // Modules may not have signatures
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "mod mymod {}".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        let result = get_signature(&db, "mymod").unwrap();
        assert!(result.signatures.is_empty());
    }
}
