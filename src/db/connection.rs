use std::path::Path;

use rusqlite::Connection;

use crate::db::schema::CREATE_SCHEMA;
use crate::error::Result;

/// Database wrapper for the rlm index.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a database at the given path and apply schema.
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
        Ok(Self { conn })
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

    /// Access the underlying connection mutably.
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
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
