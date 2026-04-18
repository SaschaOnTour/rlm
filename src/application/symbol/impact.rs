//! Impact analysis shared between CLI and MCP.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// A single location that would be impacted by changing a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactEntry {
    /// File path containing the reference.
    pub file: String,
    /// Symbol containing the reference.
    pub in_symbol: String,
    /// Line number of the reference.
    pub line: u32,
    /// Kind of reference (call, import, `type_use`).
    pub ref_kind: String,
}

/// Result of impact analysis for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactResult {
    /// The symbol being analyzed.
    pub symbol: String,
    /// List of impacted locations.
    pub impacted: Vec<ImpactEntry>,
    /// Total count of impacted locations.
    pub count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

impl ImpactResult {
    /// Number of distinct files containing at least one impacted location.
    ///
    /// This is the count the savings middleware's `SymbolFiles` cost
    /// model needs for `files_touched` — using `count` would overstate
    /// `alt_calls` whenever multiple hits share a file.
    #[must_use]
    pub fn file_count(&self) -> u64 {
        use std::collections::HashSet;
        self.impacted
            .iter()
            .map(|e| e.file.as_str())
            .collect::<HashSet<_>>()
            .len() as u64
    }
}

/// Analyze the impact of changing a symbol.
///
/// Returns all locations (file, containing symbol, line, ref kind)
/// that reference this symbol and would need updating if it changes.
pub fn analyze_impact(db: &Database, symbol: &str) -> Result<ImpactResult> {
    // Single JOIN query instead of the legacy N+1 (get_chunk_by_id +
    // get_all_files per ref). See `Database::get_refs_with_context`.
    let refs_with_ctx = db.get_refs_with_context(symbol)?;

    let impacted: Vec<ImpactEntry> = refs_with_ctx
        .into_iter()
        .map(|rc| ImpactEntry {
            file: rc.file_path,
            in_symbol: rc.containing_symbol,
            line: rc.reference.line,
            ref_kind: rc.reference.ref_kind.as_str().to_string(),
        })
        .collect();

    let count = impacted.len();
    let mut result = ImpactResult {
        symbol: symbol.to_string(),
        impacted,
        count,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;

    const TEST_FILE_BYTES: u64 = 200;
    const TEST_FILE_BYTES_MEDIUM: u64 = 100;
    const TEST_SMALL_FILE_BYTES: u64 = 50;
    const TEST_START_LINE: u32 = 1;
    const TEST_END_LINE: u32 = 5;
    const TEST_END_LINE_SHORT: u32 = 3;
    const TEST_START_BYTE: u32 = 0;
    const TEST_END_BYTE: u32 = 50;
    const TEST_END_BYTE_SMALL: u32 = 30;
    const TARGET_START_LINE: u32 = 50;
    const TARGET_END_LINE: u32 = 60;
    const TARGET_START_BYTE: u32 = 500;
    const TARGET_END_BYTE: u32 = 600;
    const CALLER1_START_LINE: u32 = 10;
    const CALLER1_END_LINE: u32 = 20;
    const CALLER1_START_BYTE: u32 = 100;
    const CALLER1_END_BYTE: u32 = 200;
    const CALLER2_START_LINE: u32 = 30;
    const CALLER2_END_LINE: u32 = 40;
    const CALLER2_START_BYTE: u32 = 300;
    const CALLER2_END_BYTE: u32 = 400;
    const CALLER1_REF_LINE: u32 = 15;
    const CALLER2_REF_LINE: u32 = 35;
    const TEST_REF_COL: u32 = 5;
    const TYPE_USER_START_LINE: u32 = 10;
    const TYPE_USER_END_LINE: u32 = 15;
    const TYPE_USER_START_BYTE: u32 = 100;
    const TYPE_USER_END_BYTE: u32 = 180;
    const TYPE_REF_COL: u32 = 18;
    const CROSS_FILE_CALLER_START_LINE: u32 = 5;
    const CROSS_FILE_CALLER_END_LINE: u32 = 10;
    const CROSS_FILE_CALLER_END_BYTE: u32 = 60;
    const CROSS_FILE_REF_LINE: u32 = 7;

    fn setup_test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn file_count_deduplicates_hits_per_file() {
        let result = ImpactResult {
            symbol: "foo".into(),
            impacted: vec![
                ImpactEntry {
                    file: "src/a.rs".into(),
                    in_symbol: "caller_a1".into(),
                    line: 10,
                    ref_kind: "call".into(),
                },
                ImpactEntry {
                    file: "src/a.rs".into(),
                    in_symbol: "caller_a2".into(),
                    line: 20,
                    ref_kind: "call".into(),
                },
                ImpactEntry {
                    file: "src/b.rs".into(),
                    in_symbol: "caller_b".into(),
                    line: 5,
                    ref_kind: "call".into(),
                },
            ],
            count: 3,
            tokens: TokenEstimate::default(),
        };
        // 3 hits across 2 distinct files.
        assert_eq!(result.count, 3);
        assert_eq!(result.file_count(), 2);
    }

    #[test]
    fn file_count_is_zero_for_empty_result() {
        let result = ImpactResult {
            symbol: "foo".into(),
            impacted: Vec::new(),
            count: 0,
            tokens: TokenEstimate::default(),
        };
        assert_eq!(result.file_count(), 0);
    }

    #[test]
    fn test_impact_empty_symbol() {
        let db = setup_test_db();
        let result = analyze_impact(&db, "nonexistent").unwrap();

        assert_eq!(result.symbol, "nonexistent");
        assert!(result.impacted.is_empty());
        assert_eq!(result.count, 0);
    }

    #[test]
    fn test_impact_basic() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/utils.rs".to_string(),
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
            ident: "helper".to_string(),
            parent: None,
            signature: Some("fn helper()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn helper() { }".to_string(),
        };
        db.insert_chunk(&target).unwrap();

        let caller1 = Chunk {
            id: 0,
            file_id,
            start_line: CALLER1_START_LINE,
            end_line: CALLER1_END_LINE,
            start_byte: CALLER1_START_BYTE,
            end_byte: CALLER1_END_BYTE,
            kind: ChunkKind::Function,
            ident: "process".to_string(),
            parent: None,
            signature: Some("fn process()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn process() { helper(); }".to_string(),
        };
        let caller1_id = db.insert_chunk(&caller1).unwrap();

        let caller2 = Chunk {
            id: 0,
            file_id,
            start_line: CALLER2_START_LINE,
            end_line: CALLER2_END_LINE,
            start_byte: CALLER2_START_BYTE,
            end_byte: CALLER2_END_BYTE,
            kind: ChunkKind::Function,
            ident: "handle".to_string(),
            parent: None,
            signature: Some("fn handle()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn handle() { helper(); }".to_string(),
        };
        let caller2_id = db.insert_chunk(&caller2).unwrap();

        let ref1 = Reference {
            id: 0,
            chunk_id: caller1_id,
            target_ident: "helper".to_string(),
            ref_kind: RefKind::Call,
            line: CALLER1_REF_LINE,
            col: TEST_REF_COL,
        };
        db.insert_ref(&ref1).unwrap();

        let ref2 = Reference {
            id: 0,
            chunk_id: caller2_id,
            target_ident: "helper".to_string(),
            ref_kind: RefKind::Call,
            line: CALLER2_REF_LINE,
            col: TEST_REF_COL,
        };
        db.insert_ref(&ref2).unwrap();

        let result = analyze_impact(&db, "helper").unwrap();

        assert_eq!(result.symbol, "helper");
        assert_eq!(result.count, 2);
        assert_eq!(result.impacted.len(), 2);

        let symbols: Vec<&str> = result
            .impacted
            .iter()
            .map(|e| e.in_symbol.as_str())
            .collect();
        assert!(symbols.contains(&"process"));
        assert!(symbols.contains(&"handle"));
    }

    #[test]
    fn test_impact_includes_ref_kind() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/types.rs".to_string(),
            "def456".to_string(),
            "rust".to_string(),
            TEST_FILE_BYTES_MEDIUM,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let type_def = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE,
            kind: ChunkKind::Struct,
            ident: "MyStruct".to_string(),
            parent: None,
            signature: Some("struct MyStruct".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "struct MyStruct { }".to_string(),
        };
        db.insert_chunk(&type_def).unwrap();

        let func = Chunk {
            id: 0,
            file_id,
            start_line: TYPE_USER_START_LINE,
            end_line: TYPE_USER_END_LINE,
            start_byte: TYPE_USER_START_BYTE,
            end_byte: TYPE_USER_END_BYTE,
            kind: ChunkKind::Function,
            ident: "use_type".to_string(),
            parent: None,
            signature: Some("fn use_type(x: MyStruct)".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn use_type(x: MyStruct) { }".to_string(),
        };
        let func_id = db.insert_chunk(&func).unwrap();

        let type_ref = Reference {
            id: 0,
            chunk_id: func_id,
            target_ident: "MyStruct".to_string(),
            ref_kind: RefKind::TypeUse,
            line: TYPE_USER_START_LINE,
            col: TYPE_REF_COL,
        };
        db.insert_ref(&type_ref).unwrap();

        let result = analyze_impact(&db, "MyStruct").unwrap();

        assert_eq!(result.count, 1);
        assert_eq!(result.impacted[0].ref_kind, "type_use");
        assert_eq!(result.impacted[0].in_symbol, "use_type");
    }

    #[test]
    fn test_impact_across_files() {
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

        let target = Chunk {
            id: 0,
            file_id: file1_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE_SHORT,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE_SMALL,
            kind: ChunkKind::Function,
            ident: "shared_fn".to_string(),
            parent: None,
            signature: Some("fn shared_fn()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn shared_fn() { }".to_string(),
        };
        db.insert_chunk(&target).unwrap();

        let caller = Chunk {
            id: 0,
            file_id: file2_id,
            start_line: CROSS_FILE_CALLER_START_LINE,
            end_line: CROSS_FILE_CALLER_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: CROSS_FILE_CALLER_END_BYTE,
            kind: ChunkKind::Function,
            ident: "consumer".to_string(),
            parent: None,
            signature: Some("fn consumer()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn consumer() { shared_fn(); }".to_string(),
        };
        let caller_id = db.insert_chunk(&caller).unwrap();

        let ref_to_target = Reference {
            id: 0,
            chunk_id: caller_id,
            target_ident: "shared_fn".to_string(),
            ref_kind: RefKind::Call,
            line: CROSS_FILE_REF_LINE,
            col: TEST_REF_COL,
        };
        db.insert_ref(&ref_to_target).unwrap();

        let result = analyze_impact(&db, "shared_fn").unwrap();

        assert_eq!(result.count, 1);
        assert_eq!(result.impacted[0].file, "src/b.rs");
        assert_eq!(result.impacted[0].in_symbol, "consumer");
    }
}
