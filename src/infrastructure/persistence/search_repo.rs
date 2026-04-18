//! Full-text search across indexed chunks.

use crate::db::Database;
use crate::error::Result;
use crate::models::chunk::Chunk;

/// FTS5-backed search over chunk content, identifiers, signatures, and doc comments.
pub trait SearchRepo {
    fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<Chunk>>;
}

impl SearchRepo for Database {
    fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<Chunk>> {
        Database::search_fts(self, query, limit)
    }
}
