//! Replay / self-healing tests for `db::migrations::mod`.
//!
//! Split out of `mod_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Fresh-DB + legacy-bootstrap
//! tests stay in `mod_tests.rs`; this file covers the idempotent replay
//! and the half-applied legacy states that must self-heal.

use super::{
    applied_versions, apply, Connection, MIGRATIONS, MIGRATION_001_BASE, MIGRATION_002_SAVINGS_V2,
    MIGRATION_003_MTIME,
};

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
fn replay_recreates_missing_fts_objects_on_legacy_db() {
    // Regression: a legacy DB has the current-shape column set
    // (doc_comment / parse_quality / alt_calls / mtime_nanos) but
    // lost its FTS virtual table or trigger — e.g. a user manually
    // dropped `chunks_fts` to reclaim space. The old bootstrap
    // marked 001 as applied based on column probes alone, so the
    // FTS objects never got recreated. After the fix, 001 always
    // runs and its `CREATE ... IF NOT EXISTS` statements put
    // every missing object back.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(MIGRATION_001_BASE).unwrap();
    conn.execute_batch(MIGRATION_002_SAVINGS_V2).unwrap();
    conn.execute_batch(MIGRATION_003_MTIME).unwrap();
    // Simulate FTS-loss post-hoc.
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS chunks_ai;\
         DROP TRIGGER IF EXISTS chunks_ad;\
         DROP TRIGGER IF EXISTS chunks_au;\
         DROP TABLE IF EXISTS chunks_fts;",
    )
    .unwrap();
    // Sanity: FTS really is gone.
    assert!(conn
        .prepare("SELECT rowid FROM chunks_fts LIMIT 0")
        .is_err());

    apply(&conn).unwrap();

    // FTS objects reinstated.
    assert!(conn.prepare("SELECT rowid FROM chunks_fts LIMIT 0").is_ok());
    let trigger_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name IN ('chunks_ai','chunks_ad','chunks_au')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(trigger_count, 3);
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
