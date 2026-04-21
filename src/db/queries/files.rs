use rusqlite::params;

use crate::domain::file::FileRecord;
use crate::error::Result;

use super::super::Database;

/// Lightweight file metadata for staleness detection.
#[derive(Debug, Clone)]
pub struct IndexedFileMeta {
    pub id: i64,
    pub path: String,
    pub hash: String,
    /// File's own mtime at index time, in nanoseconds since the Unix epoch.
    /// Compared against the on-disk mtime to short-circuit hashing when a
    /// file is unchanged. Nanosecond precision prevents same-second false
    /// negatives on modern filesystems (ext4 / NTFS / APFS / btrfs).
    pub mtime_nanos: i64,
}

impl Database {
    /// Insert or replace a file record. Returns the row ID.
    pub fn upsert_file(&self, file: &FileRecord) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO files (path, hash, lang, size_bytes) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET hash=?2, lang=?3, size_bytes=?4, indexed_at=CURRENT_TIMESTAMP",
            params![file.path, file.hash, file.lang, file.size_bytes as i64],
        )?;
        let id: i64 = self.conn().query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![file.path],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Get a file record by path.
    pub fn get_file_by_path(&self, path: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, path, hash, lang, size_bytes, mtime_nanos FROM files WHERE path = ?1",
        )?;
        let mut rows = stmt.query_map(params![path], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                hash: row.get(2)?,
                lang: row.get(3)?,
                size_bytes: row.get::<_, i64>(4)? as u64,
                mtime_nanos: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Get per-file metadata needed by staleness detection: id, path, hash,
    /// and the file's own mtime captured at index time.
    ///
    /// Used to skip SHA-256 on files whose on-disk mtime matches the stored
    /// mtime exactly (no touch since the last index).
    pub fn get_indexed_files_meta(&self) -> Result<Vec<IndexedFileMeta>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, path, hash, mtime_nanos FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |row| {
            Ok(IndexedFileMeta {
                id: row.get(0)?,
                path: row.get(1)?,
                hash: row.get(2)?,
                mtime_nanos: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Get all file records.
    pub fn get_all_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, path, hash, lang, size_bytes, mtime_nanos FROM files ORDER BY path",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                hash: row.get(2)?,
                lang: row.get(3)?,
                size_bytes: row.get::<_, i64>(4)? as u64,
                mtime_nanos: row.get(5)?,
            })
        })?;
        let mut files = Vec::new();
        for r in rows {
            files.push(r?);
        }
        Ok(files)
    }

    /// Update the stored mtime for a file after a hash-verified no-op.
    ///
    /// When staleness detects a file whose mtime has moved but whose content
    /// hash still matches (e.g. `touch`, `git checkout`), we refresh the
    /// stored mtime so the fast-path can trust it on the next call — instead
    /// of re-hashing the same stable file forever.
    pub fn update_file_mtime(&self, file_id: i64, mtime_nanos: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE files SET mtime_nanos = ?1 WHERE id = ?2",
            params![mtime_nanos, file_id],
        )?;
        Ok(())
    }

    /// Delete a file and its chunks/refs (cascade).
    pub fn delete_file(&self, file_id: i64) -> Result<()> {
        self.conn()
            .execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
        Ok(())
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
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}
