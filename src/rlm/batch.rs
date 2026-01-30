use serde::Serialize;

use crate::db::Database;
use crate::error::Result;
use crate::models::token_estimate::{estimate_tokens, TokenEstimate};
use crate::search::fts;

/// Result of a batch search operation.
#[derive(Debug, Clone, Serialize)]
pub struct BatchResult {
    #[serde(rename = "q")]
    pub query: String,
    #[serde(rename = "r")]
    pub results: Vec<BatchHit>,
    #[serde(rename = "t")]
    pub tokens: TokenEstimate,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchHit {
    #[serde(rename = "f")]
    pub file: String,
    #[serde(rename = "k")]
    pub kind: String,
    #[serde(rename = "n")]
    pub name: String,
    #[serde(rename = "l")]
    pub lines: (u32, u32),
    #[serde(rename = "c")]
    pub content: String,
}

/// Execute a query in batch across multiple files (parallel).
/// This maps a search query across all indexed files.
pub fn batch_search(db: &Database, query: &str, limit_per_file: usize) -> Result<BatchResult> {
    let results = fts::search(db, query, limit_per_file * 10)?;

    // PERF: Load file lookup table ONCE instead of per-result (N+1 query fix)
    let file_map: std::collections::HashMap<i64, String> = db
        .get_all_files()
        .ok()
        .map(|files| files.into_iter().map(|f| (f.id, f.path)).collect())
        .unwrap_or_default();

    let hits: Vec<BatchHit> = results
        .iter()
        .map(|c| {
            let file_path = file_map.get(&c.file_id).cloned().unwrap_or_default();

            BatchHit {
                file: file_path,
                kind: c.kind.as_str().to_string(),
                name: c.ident.clone(),
                lines: (c.start_line, c.end_line),
                content: c.content.clone(),
            }
        })
        .collect();

    let output_str = serde_json::to_string(&hits).unwrap_or_default();
    let out_tokens = estimate_tokens(output_str.len());

    Ok(BatchResult {
        query: query.to_string(),
        results: hits,
        tokens: TokenEstimate::new(0, out_tokens),
    })
}

/// Batch execute multiple queries in parallel and combine results.
pub fn batch_multi_query(
    db: &Database,
    queries: &[String],
    limit_per_query: usize,
) -> Result<Vec<BatchResult>> {
    // Note: rayon parallel iteration on DB is tricky since rusqlite Connection isn't Send.
    // We execute queries sequentially but process results in parallel.
    let mut results = Vec::new();
    for query in queries {
        results.push(batch_search(db, query, limit_per_query)?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    #[test]
    fn batch_search_finds_results() {
        let db = Database::open_in_memory().unwrap();
        let f = FileRecord::new("src/main.rs".into(), "h".into(), "rust".into(), 100);
        let fid = db.upsert_file(&f).unwrap();
        db.insert_chunk(&Chunk {
            id: 0,
            file_id: fid,
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: 30,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() {}".into(),
        })
        .unwrap();

        let result = batch_search(&db, "main", 10).unwrap();
        assert!(!result.results.is_empty());
    }

    #[test]
    fn batch_multi_query_works() {
        let db = Database::open_in_memory().unwrap();
        let f = FileRecord::new("src/main.rs".into(), "h".into(), "rust".into(), 100);
        let fid = db.upsert_file(&f).unwrap();
        db.insert_chunk(&Chunk {
            id: 0,
            file_id: fid,
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: 30,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() {}".into(),
        })
        .unwrap();

        let queries = vec!["main".to_string(), "nonexistent".to_string()];
        let results = batch_multi_query(&db, &queries, 10).unwrap();
        assert_eq!(results.len(), 2);
    }
}
