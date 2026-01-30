//! Impact analysis shared between CLI and MCP.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

/// A single location that would be impacted by changing a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactEntry {
    /// File path containing the reference.
    #[serde(rename = "f")]
    pub file: String,
    /// Symbol containing the reference.
    #[serde(rename = "n")]
    pub in_symbol: String,
    /// Line number of the reference.
    #[serde(rename = "l")]
    pub line: u32,
    /// Kind of reference (call, import, `type_use`).
    #[serde(rename = "k")]
    pub ref_kind: String,
}

/// Result of impact analysis for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactResult {
    /// The symbol being analyzed.
    #[serde(rename = "s")]
    pub symbol: String,
    /// List of impacted locations.
    #[serde(rename = "i")]
    pub impacted: Vec<ImpactEntry>,
    /// Total count of impacted locations.
    #[serde(rename = "c")]
    pub count: usize,
}

/// Analyze the impact of changing a symbol.
///
/// Returns all locations (file, containing symbol, line, ref kind)
/// that reference this symbol and would need updating if it changes.
pub fn analyze_impact(db: &Database, symbol: &str) -> Result<ImpactResult> {
    let refs = db.get_refs_to(symbol)?;

    let mut impacted = Vec::new();
    for r in &refs {
        if let Some(chunk) = db.get_chunk_by_id(r.chunk_id)? {
            let file = db
                .get_all_files()
                .ok()
                .and_then(|files| files.into_iter().find(|f| f.id == chunk.file_id))
                .map(|f| f.path)
                .unwrap_or_default();

            impacted.push(ImpactEntry {
                file,
                in_symbol: chunk.ident,
                line: r.line,
                ref_kind: r.ref_kind.as_str().to_string(),
            });
        }
    }

    let count = impacted.len();
    Ok(ImpactResult {
        symbol: symbol.to_string(),
        impacted,
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;

    fn setup_test_db() -> Database {
        Database::open_in_memory().unwrap()
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
            200,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let target = Chunk {
            id: 0,
            file_id,
            start_line: 50,
            end_line: 60,
            start_byte: 500,
            end_byte: 600,
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
            start_line: 10,
            end_line: 20,
            start_byte: 100,
            end_byte: 200,
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
            start_line: 30,
            end_line: 40,
            start_byte: 300,
            end_byte: 400,
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
            line: 15,
            col: 5,
        };
        db.insert_ref(&ref1).unwrap();

        let ref2 = Reference {
            id: 0,
            chunk_id: caller2_id,
            target_ident: "helper".to_string(),
            ref_kind: RefKind::Call,
            line: 35,
            col: 5,
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
            100,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let type_def = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
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
            start_line: 10,
            end_line: 15,
            start_byte: 100,
            end_byte: 180,
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
            line: 10,
            col: 18,
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
            50,
        );
        let file1_id = db.upsert_file(&file1).unwrap();

        let file2 = FileRecord::new(
            "src/b.rs".to_string(),
            "bbb".to_string(),
            "rust".to_string(),
            50,
        );
        let file2_id = db.upsert_file(&file2).unwrap();

        let target = Chunk {
            id: 0,
            file_id: file1_id,
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: 30,
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
            start_line: 5,
            end_line: 10,
            start_byte: 0,
            end_byte: 60,
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
            line: 7,
            col: 5,
        };
        db.insert_ref(&ref_to_target).unwrap();

        let result = analyze_impact(&db, "shared_fn").unwrap();

        assert_eq!(result.count, 1);
        assert_eq!(result.impacted[0].file, "src/b.rs");
        assert_eq!(result.impacted[0].in_symbol, "consumer");
    }
}
