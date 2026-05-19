use std::path::Path;

use rusqlite::Connection;

use crate::db::migrations;
use crate::db::parser_version;
use crate::error::Result;

/// Database wrapper for the rlm index.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a database at the given path and apply every
    /// pending schema migration. The migration runner owns the
    /// ancient-DB wipe (it runs inside the same `BEGIN IMMEDIATE`
    /// transaction as the replay), so concurrent opens of the same
    /// file cannot observe a half-wiped state.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;\
             PRAGMA foreign_keys=ON;\
             PRAGMA synchronous=NORMAL;\
             PRAGMA cache_size=-64000;\
             PRAGMA temp_store=MEMORY;",
        )?;
        migrations::apply(&conn)?;
        // Clears `files.hash` on parser-version mismatch so the CLI's
        // staleness check naturally re-parses every file on the next
        // read-only command (or immediately if the caller is `rlm index`).
        // MCP surfaces no warning either — agents re-index explicitly.
        parser_version::reconcile_parser_version(&conn)?;
        Ok(Self { conn })
    }

    /// Create an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        migrations::apply(&conn)?;
        parser_version::reconcile_parser_version(&conn)?;
        Ok(Self { conn })
    }

    /// Access the underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
