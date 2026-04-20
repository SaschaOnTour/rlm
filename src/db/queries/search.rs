use rusqlite::params;

use crate::domain::chunk::Chunk;
use crate::error::Result;

use super::super::Database;

impl Database {
    /// Full-text search across chunks.
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<Chunk>> {
        let mut stmt = self.conn().prepare(
            "SELECT c.id, c.file_id, c.start_line, c.end_line, c.start_byte, c.end_byte,
                    c.kind, c.ident, c.parent, c.signature, c.visibility, c.ui_ctx, c.doc_comment, c.attributes, c.content
             FROM chunks_fts f
             JOIN chunks c ON c.id = f.rowid
             WHERE chunks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        Self::map_chunks(&mut stmt, params![query, limit as i64])
    }
}
