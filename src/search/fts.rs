use crate::db::Database;
use crate::error::Result;
use crate::models::chunk::Chunk;

/// Perform a full-text search across indexed chunks.
pub fn search(db: &Database, query: &str, limit: usize) -> Result<Vec<Chunk>> {
    // Sanitize query for FTS5 (escape special characters)
    let sanitized = sanitize_fts_query(query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }
    db.search_fts(&sanitized, limit)
}

/// Sanitize a user query for FTS5 by escaping special characters
/// and converting to a prefix match for better results.
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

    #[test]
    fn sanitize_fts_query_basic() {
        let result = sanitize_fts_query("hello world");
        assert!(result.contains("\"hello\""));
        assert!(result.contains("\"world\""));
    }

    #[test]
    fn sanitize_fts_query_special_chars() {
        let result = sanitize_fts_query("fn main() {}");
        // Special chars stripped, left with fn main
        assert!(result.contains("\"fn\""));
        assert!(result.contains("\"main\""));
    }

    #[test]
    fn sanitize_fts_query_empty() {
        assert_eq!(sanitize_fts_query(""), "");
    }

    #[test]
    fn search_empty_db_returns_empty() {
        let db = Database::open_in_memory().unwrap();
        let results = search(&db, "hello", 10).unwrap();
        assert!(results.is_empty());
    }
}
