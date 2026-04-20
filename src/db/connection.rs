use std::path::Path;

use rusqlite::Connection;

use crate::db::migrations;
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
        Ok(Self { conn })
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

    /// Open an existing database, returning `RlmError::IndexNotFound` if missing.
    ///
    /// Raw opener — no auto-indexing, no staleness check. Used by commands like
    /// `verify` that need an existing index and intentionally bypass the canonical
    /// fresh-open path (which would auto-fix drift before reporting it).
    ///
    /// Distinguishes "truly missing" (→ `IndexNotFound`) from "exists but
    /// unreadable" (→ underlying IO error). `Path::exists()` would collapse
    /// both cases into a misleading `IndexNotFound`, so we use `metadata()`
    /// and match on `ErrorKind::NotFound` explicitly.
    ///
    /// Canonical full-pipeline openers:
    /// - CLI: `crate::cli::helpers::get_db` (auto-indexes + staleness check)
    /// - MCP: `crate::mcp::server_helpers::ensure_db` (staleness check only)
    // qual:allow(iosp) reason: "check-then-open is inherent to this method's purpose"
    pub fn open_required(path: &Path) -> Result<Self> {
        match std::fs::metadata(path) {
            Ok(_) => Self::open(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(crate::error::RlmError::IndexNotFound)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Create an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        migrations::apply(&conn)?;
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
