//! Search operations shared between CLI and MCP.
//!
//! Provides consistent behavior for full-text search across indexed chunks.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_tokens_str, TokenEstimate};
use crate::error::Result;
use crate::models::chunk::Chunk;

/// Approximate number of characters per token for output size estimation.
const MIN_FTS_TOKEN_LENGTH: u64 = 4;

/// Result of a full-text search.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    /// The search results.
    pub results: Vec<SearchHit>,
    /// Number of distinct files represented in `results`.
    ///
    /// Computed from the underlying chunks' `file_id` before the
    /// chunk-to-`SearchHit` mapping drops that information. Consumed
    /// by the savings middleware so the recorded `files_touched`
    /// reflects distinct files, not hit count.
    pub file_count: u64,
    /// Token usage estimate.
    pub tokens: TokenEstimate,
}

/// A single search hit.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    /// The chunk ID.
    pub id: i64,
    /// The kind of the chunk.
    pub kind: String,
    /// The symbol name.
    pub name: String,
    /// The line range [start, end].
    pub lines: (u32, u32),
    /// The content of the chunk.
    pub content: String,
}

/// Perform a full-text search across indexed chunks.
pub fn search_chunks(db: &Database, query: &str, limit: usize) -> Result<SearchResult> {
    use std::collections::HashSet;

    let results = run_fts(db, query, limit)?;

    let file_count = results
        .iter()
        .map(|c| c.file_id)
        .collect::<HashSet<_>>()
        .len() as u64;

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
        file_count,
        tokens: TokenEstimate::new(
            0,
            estimate_tokens_str(query) + total_chars as u64 / MIN_FTS_TOKEN_LENGTH,
        ),
    })
}

/// Run the FTS5 query with sanitised input, returning raw chunks.
///
/// Inlined from the former `crate::search::fts::search` when the
/// transitional `src/search/` bridge was removed. Keeps the single
/// call site (this module) and its helper in one place.
fn run_fts(db: &Database, query: &str, limit: usize) -> Result<Vec<Chunk>> {
    let sanitized = sanitize_fts_query(query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }
    db.search_fts(&sanitized, limit)
}

/// Sanitize a user query for FTS5.
///
/// Strips every character that isn't `[A-Za-z0-9_ -]`, splits on
/// whitespace, wraps each remaining term in double quotes so FTS5
/// treats it as a phrase, and joins the phrases with `OR`. Returns an
/// empty string when the input has no usable characters — the caller
/// short-circuits and returns no hits in that case.
fn sanitize_fts_query(query: &str) -> String {
    let cleaned: String = query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_' || *c == '-')
        .collect();

    let terms: Vec<String> = cleaned
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect();

    terms.join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    const TEST_FILE_BYTES: u64 = 100;
    const TEST_START_LINE: u32 = 1;
    const TEST_END_LINE: u32 = 5;
    const TEST_START_BYTE: u32 = 0;
    const TEST_END_BYTE: u32 = 50;
    const TEST_SEARCH_LIMIT: usize = 10;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn search_basic() {
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

        let result = search_chunks(&db, "search_test", TEST_SEARCH_LIMIT).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].name, "search_test");
        assert_eq!(result.results[0].kind, "fn");
        assert_eq!(result.file_count, 1);
    }

    #[test]
    fn search_no_results() {
        let db = test_db();
        let result = search_chunks(&db, "nonexistent_xyz_123", TEST_SEARCH_LIMIT).unwrap();
        assert!(result.results.is_empty());
        assert_eq!(result.file_count, 0);
    }

    #[test]
    fn sanitize_fts_query_basic() {
        let result = sanitize_fts_query("hello world");
        assert!(result.contains("\"hello\""));
        assert!(result.contains("\"world\""));
    }

    #[test]
    fn sanitize_fts_query_special_chars() {
        let result = sanitize_fts_query("fn main() {}");
        assert!(result.contains("\"fn\""));
        assert!(result.contains("\"main\""));
    }

    #[test]
    fn sanitize_fts_query_empty() {
        assert_eq!(sanitize_fts_query(""), "");
    }

    #[test]
    fn run_fts_empty_db_returns_empty() {
        let db = test_db();
        let results = run_fts(&db, "hello", TEST_SEARCH_LIMIT).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn file_count_deduplicates_hits_in_same_file() {
        let db = test_db();

        let file = FileRecord::new(
            "src/lib.rs".into(),
            "hash".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        let file_id = db.upsert_file(&file).unwrap();

        // Two distinct chunks in the SAME file, both matching the query.
        for ident in ["foo_alpha", "foo_beta"] {
            let c = Chunk {
                id: 0,
                file_id,
                start_line: TEST_START_LINE,
                end_line: TEST_END_LINE,
                start_byte: TEST_START_BYTE,
                end_byte: TEST_END_BYTE,
                kind: ChunkKind::Function,
                ident: ident.into(),
                parent: None,
                signature: None,
                visibility: None,
                ui_ctx: None,
                doc_comment: None,
                attributes: None,
                content: format!("fn {ident}() {{}}"),
            };
            db.insert_chunk(&c).unwrap();
        }

        let result = search_chunks(&db, "foo", TEST_SEARCH_LIMIT).unwrap();
        assert_eq!(result.results.len(), 2);
        // Two hits in one file → one distinct file.
        assert_eq!(result.file_count, 1);
    }
}
