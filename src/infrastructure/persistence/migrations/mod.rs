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
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "base",
        sql: MIGRATION_001_BASE,
    },
    Migration {
        version: 2,
        name: "savings_v2",
        sql: MIGRATION_002_SAVINGS_V2,
    },
    Migration {
        version: 3,
        name: "mtime",
        sql: MIGRATION_003_MTIME,
    },
];

/// Apply every pending migration to `conn`.
///
/// Creates `schema_migrations` if missing, bootstraps legacy DBs, then
/// replays any migration whose version is not yet present in the
/// tracking table. Each migration runs inside a transaction so a
/// partial failure cannot leave the DB half-migrated.
// qual:allow(iosp) reason: "integration: ensure-table + bootstrap + replay pipeline"
pub fn apply(conn: &Connection) -> Result<()> {
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
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let outcome = conn
        .execute_batch(m.sql)
        .map_err(crate::error::RlmError::from)
        .and_then(|()| mark_applied(conn, m.version, m.name));
    match outcome {
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
}
