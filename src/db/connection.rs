use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;
use crate::infrastructure::persistence::migrations;

/// Database wrapper for the rlm index.
// qual:allow(srp_lcom4) reason: "facade for 6 query modules — LCOM4 reflects domain boundaries, not SRP violation"
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a database at the given path and apply every
    /// pending schema migration. The migration runner owns the
    /// ancient-DB wipe (it runs inside the same `BEGIN IMMEDIATE`
    /// transaction as the replay), so concurrent opens of the same
    /// file cannot observe a half-wiped state.
    // qual:allow(iosp) reason: "integration: open + pragmas + migration run"
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

    #[test]
    fn ancient_schema_is_wiped_and_reseeded() {
        // Simulate an ancient rlm DB: `files` exists but without
        // `doc_comment` / `parse_quality`. A savings table of an
        // unknown old shape is also present — the wipe must drop it
        // so migration 001 recreates it with the current columns
        // rather than leaving the stale shape behind.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ancient.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE files (id INTEGER PRIMARY KEY, path TEXT);\
                 CREATE TABLE chunks (id INTEGER PRIMARY KEY);\
                 CREATE TABLE savings (id INTEGER PRIMARY KEY, stale_only_column TEXT);",
            )
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        // After wipe + migrate, the modern schema is in place.
        assert!(db
            .conn()
            .prepare("SELECT doc_comment FROM chunks LIMIT 0")
            .is_ok());
        assert!(db
            .conn()
            .prepare("SELECT alt_calls FROM savings LIMIT 0")
            .is_ok());
        assert!(db
            .conn()
            .prepare("SELECT mtime_nanos FROM files LIMIT 0")
            .is_ok());
        // The stale-only column from the pre-wipe savings table must
        // be gone — otherwise the wipe preserved the old shape.
        assert!(db
            .conn()
            .prepare("SELECT stale_only_column FROM savings LIMIT 0")
            .is_err());
    }
}
