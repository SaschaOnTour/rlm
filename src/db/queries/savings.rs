use rusqlite::params;

use crate::error::Result;

use super::super::Database;

impl Database {
    /// Record a savings entry (best-effort).
    pub fn record_savings(
        &self,
        command: &str,
        output_tokens: u64,
        alternative_tokens: u64,
        files_touched: u64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO savings (command, output_tokens, alternative_tokens, files_touched) VALUES (?1, ?2, ?3, ?4)",
            params![command, output_tokens as i64, alternative_tokens as i64, files_touched as i64],
        )?;
        Ok(())
    }

    /// Get savings breakdown by command, optionally filtered by date.
    pub fn get_savings_by_command(
        &self,
        since: Option<&str>,
    ) -> Result<Vec<(String, u64, u64, u64)>> {
        let mut stmt = self.conn().prepare(
            "SELECT command, COUNT(*), COALESCE(SUM(output_tokens), 0), COALESCE(SUM(alternative_tokens), 0) \
             FROM savings WHERE (?1 IS NULL OR created_at >= ?1) \
             GROUP BY command ORDER BY SUM(alternative_tokens) - SUM(output_tokens) DESC",
        )?;
        let rows = stmt.query_map(params![since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, i64>(3)? as u64,
            ))
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Get total size of all indexed files, optionally filtered by path prefix.
    pub fn get_scoped_file_sizes(&self, path_prefix: Option<&str>) -> Result<u64> {
        let total: i64 = if let Some(prefix) = path_prefix {
            let pattern = format!("{prefix}%");
            self.conn().query_row(
                "SELECT COALESCE(SUM(size_bytes), 0) FROM files WHERE path LIKE ?1",
                params![pattern],
                |r| r.get(0),
            )?
        } else {
            self.conn()
                .query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM files", [], |r| {
                    r.get(0)
                })?
        };
        Ok(total as u64)
    }

    /// Get total size of files involved with a symbol (definitions + references).
    pub fn get_symbol_file_sizes(&self, symbol: &str) -> Result<u64> {
        let total: i64 = self.conn().query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM (
                SELECT DISTINCT f.id, f.size_bytes FROM files f
                JOIN chunks c ON c.file_id = f.id
                WHERE c.ident = ?1
                UNION
                SELECT DISTINCT f.id, f.size_bytes FROM files f
                JOIN chunks c ON c.file_id = f.id
                JOIN refs r ON r.chunk_id = c.id
                WHERE r.target_ident = ?1
            )",
            params![symbol],
            |r| r.get(0),
        )?;
        Ok(total as u64)
    }
}

#[cfg(test)]
impl Database {
    /// Get aggregate savings totals, optionally filtered by date.
    pub(crate) fn get_savings_totals(&self, since: Option<&str>) -> Result<(u64, u64, u64)> {
        let (ops, output, alt) = self.conn().query_row(
            "SELECT COUNT(*), COALESCE(SUM(output_tokens), 0), COALESCE(SUM(alternative_tokens), 0) \
             FROM savings WHERE (?1 IS NULL OR created_at >= ?1)",
            params![since],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
        )?;
        Ok((ops as u64, output as u64, alt as u64))
    }
}
