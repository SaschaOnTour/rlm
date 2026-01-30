//! Patterns operations shared between CLI and MCP.
//!
//! Provides consistent behavior for finding similar implementations in the codebase.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;
use crate::search::fts;

/// Result of finding patterns.
#[derive(Debug, Clone, Serialize)]
pub struct PatternsResult {
    /// The query used.
    #[serde(rename = "q")]
    pub query: String,
    /// The matching patterns.
    #[serde(rename = "p")]
    pub patterns: Vec<PatternHit>,
}

/// A single pattern match.
#[derive(Debug, Clone, Serialize)]
pub struct PatternHit {
    /// The kind of the chunk.
    #[serde(rename = "k")]
    pub kind: String,
    /// The symbol name.
    #[serde(rename = "n")]
    pub name: String,
    /// The signature if available.
    #[serde(rename = "sig")]
    pub signature: Option<String>,
    /// The line count.
    #[serde(rename = "lc")]
    pub line_count: u32,
}

/// Find similar implementations in the codebase.
pub fn find_patterns(db: &Database, query: &str) -> Result<PatternsResult> {
    let results = fts::search(db, query, 20)?;

    let patterns: Vec<PatternHit> = results
        .iter()
        .map(|c| PatternHit {
            kind: c.kind.as_str().to_string(),
            name: c.ident.clone(),
            signature: c.signature.clone(),
            line_count: c.line_count(),
        })
        .collect();

    Ok(PatternsResult {
        query: query.to_string(),
        patterns,
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
    fn find_patterns_basic() {
        let db = test_db();

        let file = FileRecord::new("src/lib.rs".into(), "hash".into(), "rust".into(), 100);
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 10,
            start_byte: 0,
            end_byte: 100,
            kind: ChunkKind::Function,
            ident: "pattern_example".into(),
            parent: None,
            signature: Some("fn pattern_example(x: i32)".into()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn pattern_example(x: i32) {\n    // ...\n}".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        let result = find_patterns(&db, "pattern_example").unwrap();
        assert_eq!(result.query, "pattern_example");
        assert_eq!(result.patterns.len(), 1);
        assert_eq!(result.patterns[0].name, "pattern_example");
        assert_eq!(result.patterns[0].line_count, 10); // end_line - start_line + 1
    }
}
