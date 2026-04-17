use std::path::Path;

use rusqlite::Connection;

use crate::db::schema::{CREATE_SCHEMA, MIGRATE_FILES_MTIME, MIGRATE_SAVINGS_V2};
use crate::error::Result;

/// Database wrapper for the rlm index.
// qual:allow(srp_lcom4) reason: "facade for 6 query modules — LCOM4 reflects domain boundaries, not SRP violation"
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
        Self::migrate_files_mtime(&conn)?;
        Ok(Self { conn })
    }

    /// Apply `files.mtime_secs` migration (required, idempotent).
    ///
    /// Probes for the column before altering; treats "duplicate column" as a
    /// no-op (concurrent/replayed migration), but propagates every other
    /// failure. Returning early from `Database::open` on failure prevents a
    /// half-migrated schema from showing up as cryptic SELECT errors later.
    fn migrate_files_mtime(conn: &Connection) -> Result<()> {
        if conn.prepare("SELECT mtime_secs FROM files LIMIT 0").is_ok() {
            return Ok(());
        }
        match conn.execute(MIGRATE_FILES_MTIME, []) {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("duplicate column") => Ok(()),
            Err(e) => Err(e.into()),
        }
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

    #[test]
    fn open_required_returns_not_found_for_missing_path() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does_not_exist.db");
        match Database::open_required(&missing) {
            Err(crate::error::RlmError::IndexNotFound) => {}
            Ok(_) => panic!("missing path must not open successfully"),
            Err(e) => panic!("expected IndexNotFound, got error: {e}"),
        }
    }

    #[test]
    fn open_required_opens_existing_db() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("exists.db");
        // Create the DB first so open_required can find it.
        assert!(Database::open(&path).is_ok());
        assert!(Database::open_required(&path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn open_required_propagates_io_error_instead_of_misclassifying() {
        // Regression: `Path::exists()` returns false for permission / IO errors,
        // which would misleadingly map to IndexNotFound. `metadata()` + kind
        // matching surfaces the real error. Skipped when run as root (which
        // bypasses Unix permission checks).
        use std::os::unix::fs::PermissionsExt;

        /// Unix mode: owner+group+world no access, to deny traversal into the dir.
        const LOCKED_MODE: u32 = 0o000;
        /// Restore to standard rwxr-xr-x so `TempDir` cleanup can descend.
        const RESTORED_MODE: u32 = 0o755;

        let tmp = TempDir::new().unwrap();
        let locked = tmp.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        let inner = locked.join("db");
        std::fs::write(&inner, b"placeholder").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(LOCKED_MODE)).unwrap();

        let result = Database::open_required(&inner);

        // Restore permissions so TempDir cleanup works regardless of assertion.
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(RESTORED_MODE));

        if matches!(result, Err(crate::error::RlmError::IndexNotFound)) {
            panic!("IO error must not be classified as IndexNotFound");
        }
        // Err(other) is the expected correct behavior; Ok is only possible
        // as root (permission bypass) and is treated as inconclusive.
    }
}
