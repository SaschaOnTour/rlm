use rusqlite::params;

use crate::domain::chunk::{Chunk, ChunkKind};
use crate::error::Result;

use super::super::Database;

impl Database {
    /// Delete all chunks for a file.
    pub fn delete_chunks_for_file(&self, file_id: i64) -> Result<()> {
        self.conn()
            .execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    /// Insert a chunk. Returns the new row ID.
    pub fn insert_chunk(&self, chunk: &Chunk) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO chunks (file_id, start_line, end_line, start_byte, end_byte, kind, ident, parent, signature, visibility, ui_ctx, doc_comment, attributes, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                chunk.file_id,
                chunk.start_line,
                chunk.end_line,
                chunk.start_byte,
                chunk.end_byte,
                chunk.kind.as_str(),
                chunk.ident,
                chunk.parent,
                chunk.signature,
                chunk.visibility,
                chunk.ui_ctx,
                chunk.doc_comment,
                chunk.attributes,
                chunk.content,
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Get chunks for a file.
    pub fn get_chunks_for_file(&self, file_id: i64) -> Result<Vec<Chunk>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, file_id, start_line, end_line, start_byte, end_byte, kind, ident, parent, signature, visibility, ui_ctx, doc_comment, attributes, content
             FROM chunks WHERE file_id = ?1 ORDER BY start_line",
        )?;
        Self::map_chunks(&mut stmt, params![file_id])
    }

    /// Get a chunk by identifier (symbol name).
    pub fn get_chunks_by_ident(&self, ident: &str) -> Result<Vec<Chunk>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, file_id, start_line, end_line, start_byte, end_byte, kind, ident, parent, signature, visibility, ui_ctx, doc_comment, attributes, content
             FROM chunks WHERE ident = ?1 ORDER BY file_id, start_line",
        )?;
        Self::map_chunks(&mut stmt, params![ident])
    }

    /// Batched variant of [`get_chunks_by_ident`]. Returns chunks matching
    /// any of the given identifiers in a single query so callers like
    /// `collect_callees_with_parents` avoid the round-trip-per-ident
    /// overhead. Order is `(ident, file_id, start_line)` so consumers can
    /// group with a sequential scan.
    pub fn get_chunks_by_idents(&self, idents: &[&str]) -> Result<Vec<Chunk>> {
        let sql = build_chunks_in_idents_query(idents.len());
        let mut stmt = self.conn().prepare(&sql)?;
        Self::map_chunks(&mut stmt, rusqlite::params_from_iter(idents.iter()))
    }

    /// Get a chunk by ID.
    pub fn get_chunk_by_id(&self, id: i64) -> Result<Option<Chunk>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, file_id, start_line, end_line, start_byte, end_byte, kind, ident, parent, signature, visibility, ui_ctx, doc_comment, attributes, content
             FROM chunks WHERE id = ?1",
        )?;
        let mut chunks = Self::map_chunks(&mut stmt, params![id])?;
        Ok(chunks.pop())
    }

    /// Get all chunks.
    pub fn get_all_chunks(&self) -> Result<Vec<Chunk>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, file_id, start_line, end_line, start_byte, end_byte, kind, ident, parent, signature, visibility, ui_ctx, doc_comment, attributes, content
             FROM chunks ORDER BY file_id, start_line",
        )?;
        Self::map_chunks(&mut stmt, [])
    }

    pub(crate) fn map_chunks(
        stmt: &mut rusqlite::Statement,
        params: impl rusqlite::Params,
    ) -> Result<Vec<Chunk>> {
        let rows = stmt.query_map(params, |row| {
            Ok(Chunk {
                id: row.get(0)?,
                file_id: row.get(1)?,
                start_line: row.get(2)?,
                end_line: row.get(3)?,
                start_byte: row.get(4)?,
                end_byte: row.get(5)?,
                kind: ChunkKind::parse(row.get::<_, String>(6)?.as_str()),
                ident: row.get(7)?,
                parent: row.get(8)?,
                signature: row.get(9)?,
                visibility: row.get(10)?,
                ui_ctx: row.get(11)?,
                doc_comment: row.get(12)?,
                attributes: row.get(13)?,
                content: row.get(14)?,
            })
        })?;
        let mut chunks = Vec::new();
        for r in rows {
            chunks.push(r?);
        }
        Ok(chunks)
    }
}

/// Build the SELECT-with-`IN(?,?,…)` SQL for a `get_chunks_by_idents`
/// call. When `n == 0` we emit a never-matching predicate because
/// SQLite rejects the literal `IN ()` shape; the caller then gets an
/// empty result without a separate empty-input branch in
/// `Database::get_chunks_by_idents`.
fn build_chunks_in_idents_query(n: usize) -> String {
    if n == 0 {
        return "SELECT id, file_id, start_line, end_line, start_byte, end_byte, kind, ident, parent, signature, visibility, ui_ctx, doc_comment, attributes, content
             FROM chunks WHERE 0 = 1".to_string();
    }
    let placeholders = std::iter::repeat_n("?", n).collect::<Vec<_>>().join(",");
    format!(
        "SELECT id, file_id, start_line, end_line, start_byte, end_byte, kind, ident, parent, signature, visibility, ui_ctx, doc_comment, attributes, content
         FROM chunks WHERE ident IN ({placeholders})
         ORDER BY ident, file_id, start_line"
    )
}
