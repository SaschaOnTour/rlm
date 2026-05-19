use std::path::Path;

use rusqlite::Connection;

use crate::db::migrations;
use crate::db::parser_version;
use crate::error::Result;

/// Database wrapper for the rlm index.
///
/// Storage facade for the SQLite index — single `conn` field with
/// ~30 query methods distributed across `db::queries::{chunks, files,
/// refs, savings, search, stats}` plus the `batched` helper. LCOM4
/// naturally rises because the methods cluster by query domain
/// (chunks vs files vs refs vs …) rather than by the single shared
/// field. The structurally correct fix is per-domain Repository
/// types (ChunkRepo, FileRepo, …) — tracked as a future architecture
/// slice, not blocking 0.6.0.
// qual:allow(srp) reason: "Storage facade by design — ~30 methods distributed across db::queries::{chunks,files,refs,savings,search,stats}. Single conn field; LCOM4 rises because methods cluster by query domain, not shared field. Repo split (ChunkRepo/FileRepo/…) is a future arch slice, not 0.6.0 scope."
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
        // `busy_timeout=5000` is also the current rusqlite default; we
        // set it explicitly because rlm's `read_only_hint=true` reads
        // still write to `.rlm/` (savings counters + staleness reindex)
        // and may briefly contend with peers. If rusqlite ever drops
        // the default we don't want concurrent agent reads to start
        // reporting SQLITE_BUSY.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;\
             PRAGMA foreign_keys=ON;\
             PRAGMA synchronous=NORMAL;\
             PRAGMA cache_size=-64000;\
             PRAGMA temp_store=MEMORY;\
             PRAGMA busy_timeout=5000;",
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
