use std::path::Path;

use rusqlite::Connection;

use crate::db::schema::{CREATE_SCHEMA, MIGRATE_SAVINGS_V2};
use crate::error::Result;

/// Database wrapper for the rlm index.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a database at the given path and apply schema.
    // qual:allow(iosp) reason: "check-then-act: migration check before schema setup"
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;\
             PRAGMA foreign_keys=ON;\
             PRAGMA synchronous=NORMAL;\
             PRAGMA cache_size=-64000;\
             PRAGMA temp_store=MEMORY;",
        )?;
        // Check if schema needs migration (old DB without new columns)
        let needs_recreate = Self::needs_schema_migration(&conn);
        if needs_recreate {
            // Drop all tables and recreate with new schema
            conn.execute_batch(
                "DROP TABLE IF EXISTS chunks_fts;\
                 DROP TRIGGER IF EXISTS chunks_ai;\
                 DROP TRIGGER IF EXISTS chunks_ad;\
                 DROP TRIGGER IF EXISTS chunks_au;\
                 DROP TABLE IF EXISTS refs;\
                 DROP TABLE IF EXISTS chunks;\
                 DROP TABLE IF EXISTS files;",
            )?;
        }
        conn.execute_batch(CREATE_SCHEMA)?;
        Self::migrate_savings_v2(&conn);
        Ok(Self { conn })
    }

    /// Apply savings V2 migration (best-effort, idempotent).
    ///
    /// Probes for the last added column to avoid 4 failing ALTERs on every open.
    /// Checks `alt_calls` (last in migration order) so partial migrations are retried.
    fn migrate_savings_v2(conn: &Connection) {
        if conn
            .prepare("SELECT alt_calls FROM savings LIMIT 0")
            .is_ok()
        {
            return;
        }
        for sql in MIGRATE_SAVINGS_V2.split(';') {
            let trimmed = sql.trim();
            if !trimmed.is_empty() {
                if let Err(e) = conn.execute(trimmed, []) {
                    let msg = e.to_string();
                    if !msg.contains("duplicate column") {
                        eprintln!("warning: savings migration failed: {msg}");
                    }
                }
            }
        }
    }

    /// Check if the database needs schema migration (missing new columns).
    fn needs_schema_migration(conn: &Connection) -> bool {
        // Check if chunks table has doc_comment column
        let has_doc_comment: bool = conn
            .prepare("SELECT doc_comment FROM chunks LIMIT 0")
            .is_ok();
        let has_parse_quality: bool = conn
            .prepare("SELECT parse_quality FROM files LIMIT 0")
            .is_ok();
        // If tables exist but lack new columns, need migration
        let tables_exist: bool = conn.prepare("SELECT id FROM files LIMIT 0").is_ok();
        tables_exist && (!has_doc_comment || !has_parse_quality)
    }

    /// Open an existing database, returning `None` if the file does not exist.
    // qual:allow(iosp) reason: "check-then-open is inherent to this method's purpose"
    pub fn open_if_exists(path: &Path) -> Option<Self> {
        if path.exists() {
            Self::open(path).ok()
        } else {
            None
        }
    }

    /// Create an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(CREATE_SCHEMA)?;
        Ok(Self { conn })
    }

    /// Access the underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_in_memory_works() {
        let db = Database::open_in_memory().unwrap();
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn open_creates_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let _db = Database::open(&path).unwrap();
        assert!(path.exists());
    }
}
