//! Callgraph building shared between CLI and MCP.

use std::collections::HashSet;

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;
use crate::models::chunk::RefKind;

/// Result of building a call graph for a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct CallgraphResult {
    /// The symbol being analyzed.
    #[serde(rename = "s")]
    pub symbol: String,
    /// Functions/methods that call this symbol.
    pub callers: Vec<String>,
    /// Functions/methods that this symbol calls.
    pub callees: Vec<String>,
}

/// Build a call graph for the given symbol.
///
/// Returns the list of callers (who calls this symbol) and callees
/// (what this symbol calls).
pub fn build_callgraph(db: &Database, symbol: &str) -> Result<CallgraphResult> {
    // Find who calls this symbol (callers)
    let callers_refs = db.get_refs_to(symbol)?;

    // Find what this symbol calls (callees)
    let chunks = db.get_chunks_by_ident(symbol)?;
    let mut callees_refs = Vec::new();
    for chunk in &chunks {
        let refs = db.get_refs_from_chunk(chunk.id)?;
        callees_refs.extend(refs);
    }

    // Extract caller names from the chunks containing the references
    let caller_names: Vec<String> = callers_refs
        .iter()
        .filter_map(|r| {
            db.get_chunk_by_id(r.chunk_id)
                .ok()
                .flatten()
                .map(|c| c.ident)
        })
        .collect();

    // Extract callee names (only Call refs, deduplicated)
    let callee_names: Vec<String> = callees_refs
        .iter()
        .filter(|r| r.ref_kind == RefKind::Call)
        .map(|r| r.target_ident.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    Ok(CallgraphResult {
        symbol: symbol.to_string(),
        callers: caller_names,
        callees: callee_names,
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
    fn test_callgraph_empty_symbol() {
        let db = setup_test_db();
        let result = build_callgraph(&db, "nonexistent").unwrap();

        assert_eq!(result.symbol, "nonexistent");
        assert!(result.callers.is_empty());
        assert!(result.callees.is_empty());
    }

    #[test]
    fn test_callgraph_basic() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/lib.rs".to_string(),
            "abc123".to_string(),
            "rust".to_string(),
            100,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let caller = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
            kind: ChunkKind::Function,
            ident: "caller_fn".to_string(),
            parent: None,
            signature: Some("fn caller_fn()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn caller_fn() { target_fn(); }".to_string(),
        };
        let caller_id = db.insert_chunk(&caller).unwrap();

        let target = Chunk {
            id: 0,
            file_id,
            start_line: 10,
            end_line: 15,
            start_byte: 100,
            end_byte: 150,
            kind: ChunkKind::Function,
            ident: "target_fn".to_string(),
            parent: None,
            signature: Some("fn target_fn()".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn target_fn() { helper(); }".to_string(),
        };
        let target_id = db.insert_chunk(&target).unwrap();

        let ref_to_target = Reference {
            id: 0,
            chunk_id: caller_id,
            target_ident: "target_fn".to_string(),
            ref_kind: RefKind::Call,
            line: 3,
            col: 5,
        };
        db.insert_ref(&ref_to_target).unwrap();

        let ref_to_helper = Reference {
            id: 0,
            chunk_id: target_id,
            target_ident: "helper".to_string(),
            ref_kind: RefKind::Call,
            line: 12,
            col: 5,
        };
        db.insert_ref(&ref_to_helper).unwrap();

        let result = build_callgraph(&db, "target_fn").unwrap();

        assert_eq!(result.symbol, "target_fn");
        assert_eq!(result.callers, vec!["caller_fn"]);
        assert_eq!(result.callees, vec!["helper"]);
    }

    #[test]
    fn test_callgraph_no_callers() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/main.rs".to_string(),
            "def456".to_string(),
            "rust".to_string(),
            50,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let main_fn = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
            kind: ChunkKind::Function,
            ident: "main".to_string(),
            parent: None,
            signature: Some("fn main()".to_string()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() { println!(); }".to_string(),
        };
        let main_id = db.insert_chunk(&main_fn).unwrap();

        let ref_to_println = Reference {
            id: 0,
            chunk_id: main_id,
            target_ident: "println".to_string(),
            ref_kind: RefKind::Call,
            line: 2,
            col: 5,
        };
        db.insert_ref(&ref_to_println).unwrap();

        let result = build_callgraph(&db, "main").unwrap();

        assert_eq!(result.symbol, "main");
        assert!(result.callers.is_empty());
        assert_eq!(result.callees, vec!["println"]);
    }

    #[test]
    fn test_callgraph_filters_non_call_refs() {
        let db = setup_test_db();

        let file = FileRecord::new(
            "src/types.rs".to_string(),
            "ghi789".to_string(),
            "rust".to_string(),
            100,
        );
        let file_id = db.upsert_file(&file).unwrap();

        let func = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 80,
            kind: ChunkKind::Function,
            ident: "process".to_string(),
            parent: None,
            signature: Some("fn process(x: MyType)".to_string()),
            visibility: Some("pub".to_string()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn process(x: MyType) { helper(); }".to_string(),
        };
        let func_id = db.insert_chunk(&func).unwrap();

        let call_ref = Reference {
            id: 0,
            chunk_id: func_id,
            target_ident: "helper".to_string(),
            ref_kind: RefKind::Call,
            line: 2,
            col: 5,
        };
        db.insert_ref(&call_ref).unwrap();

        let type_ref = Reference {
            id: 0,
            chunk_id: func_id,
            target_ident: "MyType".to_string(),
            ref_kind: RefKind::TypeUse,
            line: 1,
            col: 15,
        };
        db.insert_ref(&type_ref).unwrap();

        let result = build_callgraph(&db, "process").unwrap();

        assert_eq!(result.callees.len(), 1);
        assert!(result.callees.contains(&"helper".to_string()));
        assert!(!result.callees.contains(&"MyType".to_string()));
    }
}
