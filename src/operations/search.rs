//! Search operations shared between CLI and MCP.
//!
//! Provides consistent behavior for full-text search across indexed chunks.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;
use crate::models::token_estimate::{estimate_tokens_str, TokenEstimate};
use crate::search::fts;

/// Result of a full-text search.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    /// The search results.
    #[serde(rename = "r")]
    pub results: Vec<SearchHit>,
    /// Token usage estimate.
    #[serde(rename = "t")]
    pub tokens: TokenEstimate,
}

/// A single search hit.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    /// The chunk ID.
    pub id: i64,
    /// The kind of the chunk.
    #[serde(rename = "k")]
    pub kind: String,
    /// The symbol name.
    #[serde(rename = "n")]
    pub name: String,
    /// The line range [start, end].
    #[serde(rename = "l")]
    pub lines: (u32, u32),
    /// The content of the chunk.
    #[serde(rename = "c")]
    pub content: String,
}

/// Perform a full-text search across indexed chunks.
pub fn search_chunks(db: &Database, query: &str, limit: usize) -> Result<SearchResult> {
    let results = fts::search(db, query, limit)?;

    let hits: Vec<SearchHit> = results
        .iter()
        .map(|c| SearchHit {
            id: c.id,
            kind: c.kind.as_str().to_string(),
            name: c.ident.clone(),
            lines: (c.start_line, c.end_line),
            content: c.content.clone(),
        })
        .collect();

    let total_chars: usize = hits.iter().map(|h| h.content.len()).sum();

    Ok(SearchResult {
        results: hits,
        tokens: TokenEstimate::new(0, estimate_tokens_str(query) + total_chars as u64 / 4),
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
    fn search_basic() {
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
            ident: "search_test".into(),
            parent: None,
            signature: Some("fn search_test()".into()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn search_test() { println!(\"hello\"); }".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        let result = search_chunks(&db, "search_test", 10).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].name, "search_test");
        assert_eq!(result.results[0].kind, "fn");
    }

    #[test]
    fn search_no_results() {
        let db = test_db();
        let result = search_chunks(&db, "nonexistent_xyz_123", 10).unwrap();
        assert!(result.results.is_empty());
    }
}
