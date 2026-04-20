//! Fresh-DB / bootstrap tests for `db::migrations::mod`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "mod_tests.rs"] mod tests;`.
//!
//! Replay / self-healing tests live in the sibling `mod_replay_tests.rs`.

use super::{
    applied_versions, apply, Connection, MIGRATIONS, MIGRATION_001_BASE, MIGRATION_002_SAVINGS_V2,
    MIGRATION_003_MTIME,
};

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
