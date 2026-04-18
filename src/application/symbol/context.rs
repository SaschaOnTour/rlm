//! Context building shared between CLI and MCP.

use std::collections::HashSet;

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;
use crate::models::chunk::RefKind;

use super::callgraph::{build_callgraph, CallgraphResult};
use super::SymbolQuery;

/// Complete context information for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ContextResult {
    /// The symbol being analyzed.
    pub symbol: String,
    /// Full body content of each definition.
    pub body: Vec<String>,
    /// Signatures of each definition.
    pub signatures: Vec<String>,
    /// Number of callers.
    pub caller_count: usize,
    /// Names of callees.
    pub callee_names: Vec<String>,
    /// Number of distinct files containing this symbol.
    pub file_count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Build complete context for understanding a symbol.
///
/// Returns the symbol's body content, signatures, caller count,
/// and the names of functions/methods it calls.
pub fn build_context(db: &Database, symbol: &str) -> Result<ContextResult> {
    // Get the symbol's own content
    let chunks = db.get_chunks_by_ident(symbol)?;
    let callers_refs = db.get_refs_to(symbol)?;

    // Get callees
    let mut callees = Vec::new();
    for chunk in &chunks {
        let refs = db.get_refs_from_chunk(chunk.id)?;
        callees.extend(refs);
    }

    let file_count = chunks
        .iter()
        .map(|c| c.file_id)
        .collect::<HashSet<_>>()
        .len();
    let bodies: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let sigs: Vec<String> = chunks.iter().filter_map(|c| c.signature.clone()).collect();
    let callee_names: Vec<String> = callees
        .iter()
        .filter(|r| r.ref_kind == RefKind::Call)
        .map(|r| r.target_ident.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let mut result = ContextResult {
        symbol: symbol.to_string(),
        body: bodies,
        signatures: sigs,
        caller_count: callers_refs.len(),
        callee_names,
        file_count,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Bare context query — symbol body + caller count + callee names, no
/// full callgraph expansion.
pub struct ContextQuery;

impl SymbolQuery for ContextQuery {
    type Output = ContextResult;
    const COMMAND: &'static str = "context";

    fn execute(db: &Database, symbol: &str) -> Result<Self::Output> {
        build_context(db, symbol)
    }

    fn file_count(output: &Self::Output) -> u64 {
        output.file_count as u64
    }
}

/// Combined envelope returned by [`ContextWithGraphQuery`]: the bare
/// context plus the full callgraph.
#[derive(Debug, Clone, Serialize)]
pub struct ContextWithGraph {
    pub context: ContextResult,
    pub callgraph: CallgraphResult,
}

/// Context query with full callgraph expansion.
pub struct ContextWithGraphQuery;

impl SymbolQuery for ContextWithGraphQuery {
    type Output = ContextWithGraph;
    const COMMAND: &'static str = "context";

    fn execute(db: &Database, symbol: &str) -> Result<Self::Output> {
        let context = build_context(db, symbol)?;
        let callgraph = build_callgraph(db, symbol)?;
        Ok(ContextWithGraph { context, callgraph })
    }

    fn file_count(output: &Self::Output) -> u64 {
        output.context.file_count as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;

    const TEST_FILE_BYTES: u64 = 200;
    const TEST_SMALL_FILE_BYTES: u64 = 50;
    const TEST_FILE_BYTES_MEDIUM: u64 = 100;
    const TEST_START_LINE: u32 = 1;
    const TEST_END_LINE: u32 = 5;
    const TEST_START_BYTE: u32 = 0;
    const TEST_END_BYTE: u32 = 50;
    const TEST_END_BYTE_SMALL: u32 = 30;
    const TEST_END_BYTE_MEDIUM: u32 = 40;
    const TEST_END_BYTE_LARGE: u32 = 80;
    const TARGET_START_LINE: u32 = 10;
    const TARGET_END_LINE: u32 = 20;
    const TARGET_START_BYTE: u32 = 100;
    const TARGET_END_BYTE: u32 = 300;
    const TEST_END_LINE_SHORT: u32 = 3;
    const TEST_REF_COL: u32 = 5;
    const TYPE_REF_COL: u32 = 15;
    const VALIDATE_REF_LINE: u32 = 12;
    const TRANSFORM_REF_LINE: u32 = 13;

    fn setup_test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn test_context_empty_symbol() {
        let db = setup_test_db();
        let result = build_context(&db, "nonexistent").unwrap();

        assert_eq!(result.symbol, "nonexistent");
        assert!(result.body.is_empty());
        assert!(result.signatures.is_empty());
        assert_eq!(result.caller_count, 0);
        assert!(result.callee_names.is_empty());
    }

    #[test]
    fn test_context_basic() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/lib.rs".to_string(),
            "abc123".to_string(),
            "rust".to_string(),
            TEST_FILE_BYTES,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let target = Chunk {
            id: 0,
            file_id,
            start_line: TARGET_START_LINE,
            end_line: TARGET_END_LINE,
            start_byte: TARGET_START_BYTE,
            end_byte: TARGET_END_BYTE,
            kind: ChunkKind::Function,
            ident: "process_data".to_string(),
            parent: None,
            signature: Some("fn process_data(input: &str) -> Result<String>".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: Some("Process the input data".to_string()),
            attributes: None,
            content: "fn process_data(input: &str) -> Result<String> {\n    validate(input)?;\n    transform(input)\n}".to_string(),
        };
        let target_id = db.insert_chunk(&target).unwrap();

        let caller = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE,
            kind: ChunkKind::Function,
            ident: "main".to_string(),
            parent: None,
            signature: Some("fn main()".to_string()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() { process_data(\"test\"); }".to_string(),
        };
        let caller_id = db.insert_chunk(&caller).unwrap();

        let caller_ref = Reference {
            id: 0,
            chunk_id: caller_id,
            target_ident: "process_data".to_string(),
            ref_kind: RefKind::Call,
            line: TEST_END_LINE_SHORT,
            col: TEST_REF_COL,
        };
        db.insert_ref(&caller_ref).unwrap();

        let ref1 = Reference {
            id: 0,
            chunk_id: target_id,
            target_ident: "validate".to_string(),
            ref_kind: RefKind::Call,
            line: VALIDATE_REF_LINE,
            col: TEST_REF_COL,
        };
        db.insert_ref(&ref1).unwrap();

        let ref2 = Reference {
            id: 0,
            chunk_id: target_id,
            target_ident: "transform".to_string(),
            ref_kind: RefKind::Call,
            line: TRANSFORM_REF_LINE,
            col: TEST_REF_COL,
        };
        db.insert_ref(&ref2).unwrap();

        let result = build_context(&db, "process_data").unwrap();

        assert_eq!(result.symbol, "process_data");
        assert_eq!(result.body.len(), 1);
        assert!(result.body[0].contains("process_data"));
        assert_eq!(result.signatures.len(), 1);
        assert!(result.signatures[0].contains("Result<String>"));
        assert_eq!(result.caller_count, 1);
        assert_eq!(result.callee_names.len(), 2);
        assert!(result.callee_names.contains(&"validate".to_string()));
        assert!(result.callee_names.contains(&"transform".to_string()));
        assert!(
            result.tokens.output > 0,
            "token estimate should be non-zero for non-empty result"
        );
    }

    #[test]
    fn test_context_multiple_definitions() {
        let db = setup_test_db();

        let file1 = FileRecord::new(
            "src/a.rs".to_string(),
            "aaa".to_string(),
            "rust".to_string(),
            TEST_SMALL_FILE_BYTES,
        );
        let file1_id = db.upsert_file(&file1).unwrap();

        let file2 = FileRecord::new(
            "src/b.rs".to_string(),
            "bbb".to_string(),
            "rust".to_string(),
            TEST_SMALL_FILE_BYTES,
        );
        let file2_id = db.upsert_file(&file2).unwrap();

        let chunk1 = Chunk {
            id: 0,
            file_id: file1_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE_SHORT,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_SMALL,
            kind: ChunkKind::Function,
            ident: "new".to_string(),
            parent: Some("StructA".to_string()),
            signature: Some("fn new() -> Self".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn new() -> Self { Self {} }".to_string(),
        };
        db.insert_chunk(&chunk1).unwrap();

        let chunk2 = Chunk {
            id: 0,
            file_id: file2_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE_SHORT,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_MEDIUM,
            kind: ChunkKind::Function,
            ident: "new".to_string(),
            parent: Some("StructB".to_string()),
            signature: Some("fn new(val: i32) -> Self".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn new(val: i32) -> Self { Self { val } }".to_string(),
        };
        db.insert_chunk(&chunk2).unwrap();

        let result = build_context(&db, "new").unwrap();

        assert_eq!(result.symbol, "new");
        assert_eq!(result.body.len(), 2);
        assert_eq!(result.signatures.len(), 2);
        assert_eq!(result.file_count, 2); // defined in 2 distinct files
    }

    #[test]
    fn test_context_filters_non_call_refs() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/lib.rs".to_string(),
            "xyz".to_string(),
            "rust".to_string(),
            TEST_FILE_BYTES_MEDIUM,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let func = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_LARGE,
            kind: ChunkKind::Function,
            ident: "handler".to_string(),
            parent: None,
            signature: Some("fn handler(req: Request)".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn handler(req: Request) { process(); }".to_string(),
        };
        let func_id = db.insert_chunk(&func).unwrap();

        let call_ref = Reference {
            id: 0,
            chunk_id: func_id,
            target_ident: "process".to_string(),
            ref_kind: RefKind::Call,
            line: 2,
            col: TEST_REF_COL,
        };
        db.insert_ref(&call_ref).unwrap();

        let import_ref = Reference {
            id: 0,
            chunk_id: func_id,
            target_ident: "Request".to_string(),
            ref_kind: RefKind::Import,
            line: 1,
            col: TYPE_REF_COL,
        };
        db.insert_ref(&import_ref).unwrap();

        let result = build_context(&db, "handler").unwrap();

        assert_eq!(result.callee_names.len(), 1);
        assert!(result.callee_names.contains(&"process".to_string()));
        assert!(!result.callee_names.contains(&"Request".to_string()));
    }
}
