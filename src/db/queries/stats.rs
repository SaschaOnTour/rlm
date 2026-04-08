use crate::error::Result;

use super::super::Database;

impl Database {
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

        let mut stmt = self
            .conn()
            .prepare("SELECT lang, COUNT(*) FROM files GROUP BY lang ORDER BY lang")?;
        let lang_rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let langs: Vec<_> = lang_rows.collect::<std::result::Result<Vec<_>, _>>()?;

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

    /// Verify index integrity and return a report.
    pub fn verify_integrity(&self) -> Result<VerifyReport> {
        let mut report = VerifyReport::default();

        let integrity: String = self
            .conn()
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        report.sqlite_ok = integrity == "ok";
        if !report.sqlite_ok {
            report.sqlite_error = Some(integrity);
        }

        let orphan_chunks: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_id NOT IN (SELECT id FROM files)",
            [],
            |r| r.get(0),
        )?;
        report.orphan_chunks = orphan_chunks as u64;

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
        let refs_deleted = self.conn().execute(
            "DELETE FROM refs WHERE chunk_id NOT IN (SELECT id FROM chunks)",
            [],
        )? as u64;

        let chunks_deleted = self.conn().execute(
            "DELETE FROM chunks WHERE file_id NOT IN (SELECT id FROM files)",
            [],
        )? as u64;

        Ok((chunks_deleted, refs_deleted))
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

// qual:api
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
