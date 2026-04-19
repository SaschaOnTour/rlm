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
];

/// Apply every pending migration to `conn`.
///
/// Takes a single `BEGIN IMMEDIATE` write lock for the whole sequence
/// (bootstrap + replay), so concurrent rlm processes opening the same
/// DB are serialised and cannot both observe the same "pending" set
/// and re-apply a non-idempotent migration. The lock is held across
/// `bootstrap_existing_schema` / `applied_versions` / every pending
/// `apply_one`, and released on COMMIT (or ROLLBACK on any failure).
// qual:allow(iosp) reason: "integration: lock + ensure-table + bootstrap + replay + commit"
pub fn apply(conn: &Connection) -> Result<()> {
    ensure_migrations_table(conn)?;
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

/// Body of `apply`, run with the write lock already held.
fn apply_locked(conn: &Connection) -> Result<()> {
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
/// A DB opened by the legacy inline `CREATE_SCHEMA` path already has the
/// final-shape tables but no tracking rows. Re-running the migrations
/// against it would fail (e.g. `ALTER TABLE ... ADD COLUMN` on an
/// existing column). Instead we inspect the columns and mark each
/// migration applied iff its effect is already present.
// qual:allow(iosp) reason: "integration: count check gates three column probes"
fn bootstrap_existing_schema(conn: &Connection) -> Result<()> {
    let already_tracked: i64 =
        conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))?;
    if already_tracked > 0 {
        return Ok(());
    }

    // Migration 001 is "applied" iff the current-shape base tables
    // exist. We key the detection on two columns added by the base
    // schema: chunks.doc_comment and files.parse_quality. Ancient DBs
    // that lack those columns are wiped by the caller before `apply` is
    // reached, so we never see a half-shaped schema here.
    let base_applied = conn
        .prepare("SELECT doc_comment FROM chunks LIMIT 0")
        .is_ok()
        && conn
            .prepare("SELECT parse_quality FROM files LIMIT 0")
            .is_ok();
    if !base_applied {
        // Fresh DB: no tables at all. Leave schema_migrations empty and
        // let `apply` run every migration in order.
        return Ok(());
    }
    mark_applied(conn, 1, "base")?;

    if conn
        .prepare("SELECT alt_calls FROM savings LIMIT 0")
        .is_ok()
    {
        mark_applied(conn, 2, "savings_v2")?;
    }

    if conn
        .prepare("SELECT mtime_nanos FROM files LIMIT 0")
        .is_ok()
    {
        mark_applied(conn, 3, "mtime")?;
    }

    Ok(())
}

fn applied_versions(conn: &Connection) -> Result<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations")?;
    let versions: HashSet<i64> = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(versions)
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
mod tests {
    use super::*;

    #[test]
    fn fresh_db_runs_every_migration() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn).unwrap();

        let applied = applied_versions(&conn).unwrap();
        assert_eq!(applied.len(), MIGRATIONS.len());
        for m in MIGRATIONS {
            assert!(applied.contains(&m.version), "{} not marked", m.name);
        }
    }

    #[test]
    fn replay_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn).unwrap();
        // Running again must not fail or duplicate rows.
        apply(&conn).unwrap();
        let applied = applied_versions(&conn).unwrap();
        assert_eq!(applied.len(), MIGRATIONS.len());
    }

    #[test]
    fn bootstrap_marks_legacy_db_as_fully_applied() {
        // Simulate a DB that predates the framework: the legacy inline
        // CREATE_SCHEMA produced everything migrations 1, 2 and 3
        // produce, but nothing wrote to schema_migrations.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATION_001_BASE).unwrap();
        conn.execute_batch(MIGRATION_002_SAVINGS_V2).unwrap();
        conn.execute_batch(MIGRATION_003_MTIME).unwrap();

        apply(&conn).unwrap();

        let applied = applied_versions(&conn).unwrap();
        assert!(applied.contains(&1));
        assert!(applied.contains(&2));
        assert!(applied.contains(&3));
    }

    #[test]
    fn bootstrap_detects_partial_legacy_state() {
        // Simulate a DB stuck on migrations 1 + 2 but missing 3.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATION_001_BASE).unwrap();
        conn.execute_batch(MIGRATION_002_SAVINGS_V2).unwrap();

        apply(&conn).unwrap();

        let applied = applied_versions(&conn).unwrap();
        assert!(applied.contains(&1));
        assert!(applied.contains(&2));
        assert!(applied.contains(&3));
        // Column should now exist.
        assert!(conn
            .prepare("SELECT mtime_nanos FROM files LIMIT 0")
            .is_ok());
    }

    #[test]
    fn replay_of_002_survives_partial_column_set() {
        // Regression: a DB where migration 002 is half-applied (some of
        // the four savings columns already exist because an older rlm
        // ran the pre-framework probe-and-alter logic and crashed
        // mid-way). `bootstrap_existing_schema` only marks 002 as
        // applied if `alt_calls` exists, so here 002 is pending. The
        // replay must not abort on the first `ALTER TABLE ... ADD
        // COLUMN` that hits an existing column.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATION_001_BASE).unwrap();
        // Partial 002: first two columns present, last two missing.
        conn.execute_batch(
            "ALTER TABLE savings ADD COLUMN rlm_input_tokens INTEGER NOT NULL DEFAULT 0;\
             ALTER TABLE savings ADD COLUMN alt_input_tokens INTEGER NOT NULL DEFAULT 0;",
        )
        .unwrap();

        apply(&conn).unwrap();

        let applied = applied_versions(&conn).unwrap();
        assert!(applied.contains(&2));
        // All four columns present after replay.
        for col in [
            "rlm_input_tokens",
            "alt_input_tokens",
            "rlm_calls",
            "alt_calls",
        ] {
            let sql = format!("SELECT {col} FROM savings LIMIT 0");
            assert!(conn.prepare(&sql).is_ok(), "{col} missing");
        }
    }
}
