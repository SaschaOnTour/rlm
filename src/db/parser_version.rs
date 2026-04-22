//! Parser-version reconciliation on DB open (task #118).
//!
//! File-content changes are detected by SHA-256 hash comparison at
//! index time, which is fast and correct. Parser-vocabulary changes
//! (new [`crate::domain::chunk::ChunkKind`], new ref kinds, richer
//! chunk extraction for features that weren't captured before) are
//! invisible to that check: a file unchanged since the last index
//! still has the same hash, so rlm skips it and never gets the new
//! chunks.
//!
//! This module stamps the current parser version into the DB at open
//! time and triggers a full reindex (by clearing every file's stored
//! hash) when the stamped version does not match what the running
//! binary produces.

use rusqlite::{params, Connection};

use crate::error::Result;

/// Current parser-output "version" baked into the binary. Bump when any
/// parser starts producing new / different chunks or refs so that DBs
/// indexed by older binaries auto-reindex on next open. Release version
/// is convenient but arbitrary — only equality matters.
pub const CURRENT_PARSER_VERSION: &str = "0.5.0";

const META_KEY: &str = "parser_version";

/// Outcome of [`reconcile_parser_version`] — useful for callers that
/// want to emit a warning or trigger a user-facing reindex message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParserVersionState {
    /// No prior row; the current version was just stamped.
    Fresh,
    /// Stored value equals `CURRENT_PARSER_VERSION`; nothing to do.
    UpToDate,
    /// Stored value differed (typically an older rlm release). Every
    /// file's `hash` **and** `mtime_nanos` have been cleared so the
    /// next `rlm index` forces a full rehash-and-reparse with the
    /// current binary, bypassing the mtime fast-path in staleness.
    UpgradedFrom(String),
}

/// Read the stored parser version, if any. Returns `None` for a fresh
/// DB (no row yet) or when the meta table has no such key.
pub fn stored_parser_version(conn: &Connection) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
    let mut rows = stmt.query(params![META_KEY])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get::<_, String>(0)?)),
        None => Ok(None),
    }
}

/// Reconcile the DB's stored parser version against the binary's
/// [`CURRENT_PARSER_VERSION`]. Takes a `BEGIN IMMEDIATE` write lock so
/// concurrent opens cannot observe a half-reconciled state.
///
/// Behaviour matrix:
/// - no stored row → insert current, return [`Fresh`].
/// - stored equals current → return [`UpToDate`] without writing.
/// - stored differs → clear `files.hash`, overwrite the stored value,
///   return [`UpgradedFrom`] with the prior value.
///
/// [`Fresh`]: ParserVersionState::Fresh
/// [`UpToDate`]: ParserVersionState::UpToDate
/// [`UpgradedFrom`]: ParserVersionState::UpgradedFrom
pub fn reconcile_parser_version(conn: &Connection) -> Result<ParserVersionState> {
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    match reconcile_locked(conn) {
        Ok(state) => {
            conn.execute_batch("COMMIT;")?;
            Ok(state)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(e)
        }
    }
}

fn reconcile_locked(conn: &Connection) -> Result<ParserVersionState> {
    match stored_parser_version(conn)? {
        None => {
            stamp_current(conn)?;
            Ok(ParserVersionState::Fresh)
        }
        Some(v) if v == CURRENT_PARSER_VERSION => Ok(ParserVersionState::UpToDate),
        Some(prev) => {
            clear_file_staleness_markers(conn)?;
            stamp_current(conn)?;
            Ok(ParserVersionState::UpgradedFrom(prev))
        }
    }
}

fn stamp_current(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
        params![META_KEY, CURRENT_PARSER_VERSION],
    )?;
    Ok(())
}

/// Reset every file's staleness markers so the next staleness pass
/// cannot short-circuit: both `hash` (the slow-path comparison) and
/// `mtime_nanos` (the fast-path equality gate in
/// `application::index::staleness`) are cleared. Clearing only one
/// is not enough — the fast-path returns before it reaches the hash.
fn clear_file_staleness_markers(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE files SET hash = '', mtime_nanos = 0", [])?;
    Ok(())
}

#[cfg(test)]
#[path = "parser_version_tests.rs"]
mod tests;
