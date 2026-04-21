//! Numbered SQL migrations for the rlm index database.
//!
//! Each migration is a small SQL file in this directory pulled in at
//! compile time via `include_str!`, paired with a monotonically
//! increasing integer version. A `schema_migrations` table tracks which
//! versions have been applied, so `apply` is effectively a replay of
//! anything still pending on the open database.
//!
//! The bootstrap layer handles pre-migration databases (ones opened by
//! the legacy `CREATE_SCHEMA` path that predated this framework): if
//! the current-shape tables already exist but no `schema_migrations`
//! table does, we inspect the schema shape (presence of key columns)
//! and seed the tracking table with the migrations that are
//! effectively already applied. Nothing gets re-run.

use std::collections::HashSet;

use rusqlite::{params, Connection};

use crate::error::Result;

const MIGRATION_001_BASE: &str = include_str!("001_base.sql");
const MIGRATION_002_SAVINGS_V2: &str = include_str!("002_savings_v2.sql");
const MIGRATION_003_MTIME: &str = include_str!("003_mtime.sql");
const MIGRATION_004_META: &str = include_str!("004_meta.sql");

/// A single migration: its monotonic version, short human-readable
/// name (used in the `schema_migrations.name` column), and SQL body.
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
    /// If true, the SQL is a sequence of idempotent single statements
    /// (`ALTER TABLE ... ADD COLUMN`) that the runner executes one at a
    /// time so it can tolerate `duplicate column` errors. Needed when
    /// replaying a legacy DB whose schema is partially upgraded (e.g.
    /// some of migration 002's four columns already exist because an
    /// older rlm ran the pre-framework probe-and-alter logic but
    /// crashed mid-way). Migrations that include multi-statement
    /// constructs like FTS triggers (001) must not use this mode —
    /// splitting on `;` would cut `BEGIN ... END;` trigger bodies in
    /// half.
    tolerate_duplicate_column: bool,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "base",
        sql: MIGRATION_001_BASE,
        tolerate_duplicate_column: false,
    },
    Migration {
        version: 2,
        name: "savings_v2",
        sql: MIGRATION_002_SAVINGS_V2,
        tolerate_duplicate_column: true,
    },
    Migration {
        version: 3,
        name: "mtime",
        sql: MIGRATION_003_MTIME,
        tolerate_duplicate_column: true,
    },
    Migration {
        version: 4,
        name: "meta",
        sql: MIGRATION_004_META,
        tolerate_duplicate_column: false,
    },
];

/// Apply every pending migration to `conn`.
///
/// Takes a single `BEGIN IMMEDIATE` write lock for the whole sequence
/// (bootstrap + replay), so concurrent rlm processes opening the same
/// DB are serialised and cannot both observe the same "pending" set
/// and re-apply a non-idempotent migration. The lock is held across
/// `bootstrap_existing_schema` / `applied_versions` / every pending
/// `apply_one`, and released on COMMIT (or ROLLBACK on any failure).
pub fn apply(conn: &Connection) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    match apply_locked(conn) {
        Ok(()) => {
            conn.execute_batch("COMMIT;")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(e)
        }
    }
}

/// Body of `apply`, run with the write lock already held. The
/// ancient-DB wipe runs here too (not in `Database::open` before
/// `BEGIN IMMEDIATE`) so concurrent opens can't race against a
/// half-wiped schema: one process holds the lock until every drop,
/// bootstrap and replay has committed. Creating `schema_migrations`
/// also lives inside the lock, so the bootstrap read that follows
/// sees the same transactional snapshot as the replay loop.
fn apply_locked(conn: &Connection) -> Result<()> {
    if needs_ancient_wipe(conn) {
        wipe_ancient_schema(conn)?;
    }
    ensure_migrations_table(conn)?;
    bootstrap_existing_schema(conn)?;
    let applied = applied_versions(conn)?;
    for m in MIGRATIONS {
        if applied.contains(&m.version) {
            continue;
        }
        apply_one(conn, m)?;
    }
    Ok(())
}

/// Detect databases that predate even migration 001's shape.
///
/// A pre-base rlm DB has a `files` row but lacks `chunks.doc_comment`
/// or `files.parse_quality`. We can't express that schema jump as a
/// cumulative migration without risking data in a file no one still
/// has; the pragmatic choice is to wipe and re-index, matching the
/// behaviour of every prior rlm release.
fn needs_ancient_wipe(conn: &Connection) -> bool {
    let tables_exist = conn.prepare("SELECT id FROM files LIMIT 0").is_ok();
    if !tables_exist {
        return false;
    }
    let has_doc_comment = conn
        .prepare("SELECT doc_comment FROM chunks LIMIT 0")
        .is_ok();
    let has_parse_quality = conn
        .prepare("SELECT parse_quality FROM files LIMIT 0")
        .is_ok();
    !has_doc_comment || !has_parse_quality
}

fn wipe_ancient_schema(conn: &Connection) -> Result<()> {
    // Drop `savings` too: a pre-V1 DB may carry a savings table of
    // unknown shape, and migration 001's `CREATE TABLE IF NOT EXISTS`
    // would leave that stale shape in place; migration 002's
    // ALTER TABLE ADD COLUMN statements would then run against it and
    // produce a mixed schema. Dropping lets 001 recreate it cleanly.
    conn.execute_batch(
        "DROP TABLE IF EXISTS chunks_fts;\
         DROP TRIGGER IF EXISTS chunks_ai;\
         DROP TRIGGER IF EXISTS chunks_ad;\
         DROP TRIGGER IF EXISTS chunks_au;\
         DROP TABLE IF EXISTS refs;\
         DROP TABLE IF EXISTS chunks;\
         DROP TABLE IF EXISTS files;\
         DROP TABLE IF EXISTS savings;\
         DROP TABLE IF EXISTS schema_migrations;",
    )?;
    Ok(())
}

fn ensure_migrations_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT DEFAULT CURRENT_TIMESTAMP
        );",
    )?;
    Ok(())
}

/// Seed `schema_migrations` for databases that predate this framework.
///
/// A DB opened by the legacy inline `CREATE_SCHEMA` path already has
/// the final-shape tables but no tracking rows. Blindly replaying the
/// `ALTER TABLE ... ADD COLUMN` statements in migrations 002 / 003
/// would fail on the first column that already exists, so we mark
/// each of them applied iff its effect is already present.
///
/// Migration 001 is intentionally never marked here: its SQL is all
/// `CREATE TABLE / INDEX / VIRTUAL TABLE / TRIGGER IF NOT EXISTS`, so
/// replaying it is safe and idempotent. Running it unconditionally
/// means a legacy DB that lost the `chunks_fts` virtual table, any
/// index, or any of the FTS sync triggers (e.g. from a manual drop)
/// gets those objects recreated on the next open — a narrow-probe
/// "is 001 applied?" check could miss the missing object.
fn bootstrap_existing_schema(conn: &Connection) -> Result<()> {
    let already_tracked: i64 =
        conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))?;
    if already_tracked > 0 {
        return Ok(());
    }

    if conn
        .prepare("SELECT alt_calls FROM savings LIMIT 0")
        .is_ok()
    {
        let m = &MIGRATIONS[1];
        mark_applied(conn, m.version, m.name)?;
    }

    if conn
        .prepare("SELECT mtime_nanos FROM files LIMIT 0")
        .is_ok()
    {
        let m = &MIGRATIONS[2];
        mark_applied(conn, m.version, m.name)?;
    }

    Ok(())
}

fn applied_versions(conn: &Connection) -> Result<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations")?;
    // Collecting into Result propagates per-row errors instead of
    // swallowing them via filter_map(Ok). A corrupted schema_migrations
    // row (unexpected type, truncated page) used to silently drop its
    // version and make the runner re-execute a migration that is
    // already applied; bubbling the error aborts the open instead.
    let versions = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<std::result::Result<Vec<i64>, _>>()?;
    Ok(versions.into_iter().collect())
}

fn apply_one(conn: &Connection, m: &Migration) -> Result<()> {
    // The outer transaction in `apply` provides atomicity; here we
    // just execute the SQL (all-at-once for multi-statement constructs
    // like FTS triggers, or statement-by-statement for idempotent
    // ALTER TABLE sequences so a partially-upgraded legacy DB can
    // finish cleanly).
    if m.tolerate_duplicate_column {
        for stmt in m.sql.split(';') {
            // Skip chunks that contain no executable SQL once line
            // comments and whitespace are stripped. Checking only the
            // first trimmed char would drop statements preceded by a
            // comment block (as every migration file has at the top).
            if is_blank_sql(stmt) {
                continue;
            }
            match conn.execute_batch(stmt) {
                Ok(()) => {}
                Err(e) if is_duplicate_column(&e) => {
                    // Column already added by an earlier partial run.
                }
                Err(e) => return Err(e.into()),
            }
        }
    } else {
        conn.execute_batch(m.sql)?;
    }
    mark_applied(conn, m.version, m.name)?;
    Ok(())
}

/// Match rusqlite's duplicate-column error without coupling to its
/// internal error code constants — the text is stable across versions
/// and the same probe the pre-framework savings migration used.
fn is_duplicate_column(e: &rusqlite::Error) -> bool {
    e.to_string().contains("duplicate column")
}

/// True if `stmt` consists only of whitespace and `-- line comments`.
fn is_blank_sql(stmt: &str) -> bool {
    stmt.lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with("--"))
}

fn mark_applied(conn: &Connection, version: i64, name: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (?, ?)",
        params![version, name],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "mod_replay_tests.rs"]
mod replay_tests;
#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
