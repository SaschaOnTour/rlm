use rusqlite::{params, Transaction};

use crate::error::Result;
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::models::file::FileRecord;

use super::Database;

impl Database {
    // ─── File operations ───

    /// Insert or replace a file record. Returns the row ID.
    pub fn upsert_file(&self, file: &FileRecord) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO files (path, hash, lang, size_bytes) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET hash=?2, lang=?3, size_bytes=?4, indexed_at=CURRENT_TIMESTAMP",
            params![file.path, file.hash, file.lang, file.size_bytes as i64],
        )?;
        // last_insert_rowid() is unreliable for ON CONFLICT DO UPDATE,
        // so always query back the actual ID.
        let id: i64 = self.conn().query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![file.path],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Get a file record by path.
    pub fn get_file_by_path(&self, path: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, path, hash, lang, size_bytes FROM files WHERE path = ?1")?;
        let mut rows = stmt.query_map(params![path], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                hash: row.get(2)?,
                lang: row.get(3)?,
                size_bytes: row.get::<_, i64>(4)? as u64,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Get all file records.
    pub fn get_all_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, path, hash, lang, size_bytes FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                hash: row.get(2)?,
                lang: row.get(3)?,
                size_bytes: row.get::<_, i64>(4)? as u64,
            })
        })?;
        let mut files = Vec::new();
        for r in rows {
            files.push(r?);
        }
        Ok(files)
    }

    /// Delete a file and its chunks/refs (cascade).
    pub fn delete_file(&self, file_id: i64) -> Result<()> {
        self.conn()
            .execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
        Ok(())
    }

    // ─── Chunk operations ───

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

    fn map_chunks(
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

    // ─── Reference operations ───

    /// Insert a reference.
    pub fn insert_ref(&self, reference: &Reference) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO refs (chunk_id, target_ident, ref_kind, line, col) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                reference.chunk_id,
                reference.target_ident,
                reference.ref_kind.as_str(),
                reference.line,
                reference.col,
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Find all references to a given identifier.
    pub fn get_refs_to(&self, target_ident: &str) -> Result<Vec<Reference>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, chunk_id, target_ident, ref_kind, line, col FROM refs WHERE target_ident = ?1",
        )?;
        let rows = stmt.query_map(params![target_ident], |row| {
            Ok(Reference {
                id: row.get(0)?,
                chunk_id: row.get(1)?,
                target_ident: row.get(2)?,
                ref_kind: RefKind::parse(row.get::<_, String>(3)?.as_str()),
                line: row.get(4)?,
                col: row.get(5)?,
            })
        })?;
        let mut refs = Vec::new();
        for r in rows {
            refs.push(r?);
        }
        Ok(refs)
    }

    /// Get all references from a chunk.
    pub fn get_refs_from_chunk(&self, chunk_id: i64) -> Result<Vec<Reference>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, chunk_id, target_ident, ref_kind, line, col FROM refs WHERE chunk_id = ?1",
        )?;
        let rows = stmt.query_map(params![chunk_id], |row| {
            Ok(Reference {
                id: row.get(0)?,
                chunk_id: row.get(1)?,
                target_ident: row.get(2)?,
                ref_kind: RefKind::parse(row.get::<_, String>(3)?.as_str()),
                line: row.get(4)?,
                col: row.get(5)?,
            })
        })?;
        let mut refs = Vec::new();
        for r in rows {
            refs.push(r?);
        }
        Ok(refs)
    }

    /// Get all references for a file (via its chunks, including file-level refs with `chunk_id` from any chunk in the file).
    pub fn get_refs_for_file(&self, file_id: i64) -> Result<Vec<Reference>> {
        let mut stmt = self.conn().prepare(
            "SELECT r.id, r.chunk_id, r.target_ident, r.ref_kind, r.line, r.col
             FROM refs r
             JOIN chunks c ON r.chunk_id = c.id
             WHERE c.file_id = ?1
             ORDER BY r.line",
        )?;
        let rows = stmt.query_map(params![file_id], |row| {
            Ok(Reference {
                id: row.get(0)?,
                chunk_id: row.get(1)?,
                target_ident: row.get(2)?,
                ref_kind: RefKind::parse(row.get::<_, String>(3)?.as_str()),
                line: row.get(4)?,
                col: row.get(5)?,
            })
        })?;
        let mut refs = Vec::new();
        for r in rows {
            refs.push(r?);
        }
        Ok(refs)
    }

    // ─── FTS5 search ───

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

    // ─── Statistics ───

    /// Get index statistics.
    pub fn stats(&self) -> Result<IndexStats> {
        let file_count: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let chunk_count: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
        let ref_count: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM refs", [], |r| r.get(0))?;
        let total_bytes: i64 =
            self.conn()
                .query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM files", [], |r| {
                    r.get(0)
                })?;

        // Language breakdown
        let mut stmt = self
            .conn()
            .prepare("SELECT lang, COUNT(*) FROM files GROUP BY lang ORDER BY lang")?;
        let lang_rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut langs = Vec::new();
        for r in lang_rows {
            langs.push(r?);
        }

        // Index age: oldest and newest indexed_at
        let oldest_indexed: Option<String> = self
            .conn()
            .query_row("SELECT MIN(indexed_at) FROM files", [], |r| r.get(0))
            .ok();

        let newest_indexed: Option<String> = self
            .conn()
            .query_row("SELECT MAX(indexed_at) FROM files", [], |r| r.get(0))
            .ok();

        Ok(IndexStats {
            file_count: file_count as u64,
            chunk_count: chunk_count as u64,
            ref_count: ref_count as u64,
            total_bytes: total_bytes as u64,
            languages: langs,
            oldest_indexed,
            newest_indexed,
        })
    }

    /// Set the parse quality for a file.
    pub fn set_file_parse_quality(&self, file_id: i64, quality: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE files SET parse_quality = ?1 WHERE id = ?2",
            params![quality, file_id],
        )?;
        Ok(())
    }

    /// Get files with parse quality issues.
    pub fn get_files_with_quality_issues(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn().prepare(
            "SELECT path, parse_quality FROM files WHERE parse_quality != 'complete' ORDER BY path",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Begin a transaction for batch operations.
    pub fn begin_transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self.conn_mut().transaction()?)
    }

    /// Verify index integrity and return a report.
    pub fn verify_integrity(&self) -> Result<VerifyReport> {
        let mut report = VerifyReport::default();

        // 1. SQLite integrity check
        let integrity: String = self
            .conn()
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        report.sqlite_ok = integrity == "ok";
        if !report.sqlite_ok {
            report.sqlite_error = Some(integrity);
        }

        // 2. Orphan chunks (file_id points to non-existent file)
        let orphan_chunks: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_id NOT IN (SELECT id FROM files)",
            [],
            |r| r.get(0),
        )?;
        report.orphan_chunks = orphan_chunks as u64;

        // 3. Orphan refs (chunk_id points to non-existent chunk)
        let orphan_refs: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM refs WHERE chunk_id NOT IN (SELECT id FROM chunks)",
            [],
            |r| r.get(0),
        )?;
        report.orphan_refs = orphan_refs as u64;

        Ok(report)
    }

    /// Fix orphan chunks and refs by deleting them.
    pub fn fix_orphans(&self) -> Result<(u64, u64)> {
        // Delete orphan refs first (they reference chunks)
        let refs_deleted = self.conn().execute(
            "DELETE FROM refs WHERE chunk_id NOT IN (SELECT id FROM chunks)",
            [],
        )? as u64;

        // Delete orphan chunks
        let chunks_deleted = self.conn().execute(
            "DELETE FROM chunks WHERE file_id NOT IN (SELECT id FROM files)",
            [],
        )? as u64;

        Ok((chunks_deleted, refs_deleted))
    }

    /// Get all indexed file paths.
    pub fn get_all_file_paths(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT path FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut paths = Vec::new();
        for r in rows {
            paths.push(r?);
        }
        Ok(paths)
    }

    /// Delete a file by path.
    pub fn delete_file_by_path(&self, path: &str) -> Result<bool> {
        let rows = self
            .conn()
            .execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(rows > 0)
    }
}

/// Report from index integrity verification.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct VerifyReport {
    /// Whether `SQLite` integrity check passed.
    pub sqlite_ok: bool,
    /// `SQLite` error message if integrity check failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sqlite_error: Option<String>,
    /// Number of orphan chunks (`file_id` points to deleted file).
    pub orphan_chunks: u64,
    /// Number of orphan refs (`chunk_id` points to deleted chunk).
    pub orphan_refs: u64,
    /// Number of indexed files that no longer exist on disk.
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub missing_files: u64,
    /// Paths of missing files.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_file_paths: Vec<String>,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

impl VerifyReport {
    /// Returns true if all checks passed.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.sqlite_ok
            && self.orphan_chunks == 0
            && self.orphan_refs == 0
            && self.missing_files == 0
    }
}

/// Index statistics.
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub file_count: u64,
    pub chunk_count: u64,
    pub ref_count: u64,
    pub total_bytes: u64,
    pub languages: Vec<(String, i64)>,
    /// Oldest `indexed_at` timestamp (ISO 8601).
    pub oldest_indexed: Option<String>,
    /// Newest `indexed_at` timestamp (ISO 8601).
    pub newest_indexed: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn sample_file() -> FileRecord {
        FileRecord::new("src/main.rs".into(), "abc123".into(), "rust".into(), 1024)
    }

    fn sample_chunk(file_id: i64) -> Chunk {
        Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 10,
            start_byte: 0,
            end_byte: 200,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: Some("fn main()".into()),
            visibility: Some("pub".into()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() {\n    println!(\"hello\");\n}".into(),
        }
    }

    #[test]
    fn upsert_file_and_retrieve() {
        let db = test_db();
        let f = sample_file();
        let id = db.upsert_file(&f).unwrap();
        assert!(id > 0);
        let got = db.get_file_by_path("src/main.rs").unwrap().unwrap();
        assert_eq!(got.hash, "abc123");
    }

    #[test]
    fn upsert_file_updates_existing() {
        let db = test_db();
        let f = sample_file();
        db.upsert_file(&f).unwrap();
        let f2 = FileRecord::new("src/main.rs".into(), "def456".into(), "rust".into(), 2048);
        db.upsert_file(&f2).unwrap();
        let got = db.get_file_by_path("src/main.rs").unwrap().unwrap();
        assert_eq!(got.hash, "def456");
    }

    #[test]
    fn insert_chunk_and_retrieve() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        let cid = db.insert_chunk(&c).unwrap();
        assert!(cid > 0);
        let chunks = db.get_chunks_for_file(fid).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ident, "main");
    }

    #[test]
    fn search_fts_finds_content() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        db.insert_chunk(&c).unwrap();
        let results = db.search_fts("main", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].ident, "main");
    }

    #[test]
    fn insert_ref_and_find() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        let cid = db.insert_chunk(&c).unwrap();
        let r = Reference {
            id: 0,
            chunk_id: cid,
            target_ident: "println".into(),
            ref_kind: RefKind::Call,
            line: 2,
            col: 4,
        };
        db.insert_ref(&r).unwrap();
        let refs = db.get_refs_to("println").unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].ref_kind, RefKind::Call);
    }

    #[test]
    fn stats_returns_counts() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        db.insert_chunk(&c).unwrap();
        let stats = db.stats().unwrap();
        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.chunk_count, 1);
        assert_eq!(stats.languages.len(), 1);
    }

    #[test]
    fn delete_file_cascades() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        db.insert_chunk(&c).unwrap();
        db.delete_file(fid).unwrap();
        let files = db.get_all_files().unwrap();
        assert!(files.is_empty());
        let chunks = db.get_all_chunks().unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn get_chunks_by_ident_works() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        db.insert_chunk(&c).unwrap();
        let chunks = db.get_chunks_by_ident("main").unwrap();
        assert_eq!(chunks.len(), 1);
    }
}
