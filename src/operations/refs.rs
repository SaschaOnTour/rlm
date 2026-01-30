//! Refs operations shared between CLI and MCP.
//!
//! Provides consistent behavior for finding all usages/call sites of a symbol.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

/// Result of finding all references to a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct RefsResult {
    /// The symbol name.
    #[serde(rename = "s")]
    pub symbol: String,
    /// The list of references.
    #[serde(rename = "r")]
    pub refs: Vec<RefHit>,
    /// Total count of references.
    #[serde(rename = "c")]
    pub count: usize,
}

/// A single reference hit.
#[derive(Debug, Clone, Serialize)]
pub struct RefHit {
    /// The kind of reference (call, import, `type_use`).
    #[serde(rename = "k")]
    pub kind: String,
    /// The line number.
    #[serde(rename = "l")]
    pub line: u32,
    /// The column number.
    pub col: u32,
    /// The chunk ID containing this reference.
    /// Note: Using `cid` for consistency (was inconsistent between CLI/MCP before).
    #[serde(rename = "cid")]
    pub chunk_id: i64,
}

/// Find all references (usages/call sites) of a symbol.
pub fn get_refs(db: &Database, symbol: &str) -> Result<RefsResult> {
    let refs = db.get_refs_to(symbol)?;

    let hits: Vec<RefHit> = refs
        .iter()
        .map(|r| RefHit {
            kind: r.ref_kind.as_str().to_string(),
            line: r.line,
            col: r.col,
            chunk_id: r.chunk_id,
        })
        .collect();

    let count = hits.len();

    Ok(RefsResult {
        symbol: symbol.to_string(),
        refs: hits,
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn get_refs_basic() {
        let db = test_db();

        let file = FileRecord::new("src/lib.rs".into(), "hash".into(), "rust".into(), 100);
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
            kind: ChunkKind::Function,
            ident: "caller".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn caller() { foo(); }".into(),
        };
        let chunk_id = db.insert_chunk(&chunk).unwrap();

        let reference = Reference {
            id: 0,
            chunk_id,
            target_ident: "foo".into(),
            ref_kind: RefKind::Call,
            line: 1,
            col: 14,
        };
        db.insert_ref(&reference).unwrap();

        let result = get_refs(&db, "foo").unwrap();
        assert_eq!(result.symbol, "foo");
        assert_eq!(result.count, 1);
        assert_eq!(result.refs[0].kind, "call");
        assert_eq!(result.refs[0].line, 1);
        assert_eq!(result.refs[0].col, 14);
    }

    #[test]
    fn get_refs_empty() {
        let db = test_db();
        let result = get_refs(&db, "nonexistent").unwrap();
        assert_eq!(result.count, 0);
        assert!(result.refs.is_empty());
    }
}
