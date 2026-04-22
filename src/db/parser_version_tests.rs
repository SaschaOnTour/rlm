//! Tests for `parser_version.rs` (task #118).
//!
//! Covers fresh stamp, match no-op, mismatch reconcile (hash clear +
//! stored-value update), and the stored value's observable state after
//! each transition.

use super::{
    reconcile_parser_version, stored_parser_version, ParserVersionState, CURRENT_PARSER_VERSION,
};
use crate::db::Database;
use crate::domain::file::FileRecord;

fn make_indexed_db(hashes: &[&str]) -> Database {
    let db = Database::open_in_memory().unwrap();
    for (i, h) in hashes.iter().enumerate() {
        let f = FileRecord::new(format!("file{i}.rs"), (*h).to_string(), "rust".into(), 100);
        let id = db.upsert_file(&f).unwrap();
        // `upsert_file` ignores the `mtime_nanos` field on insert —
        // production indexing sets it via a follow-up update. Seed a
        // non-zero value the same way so the staleness fast-path
        // *would* short-circuit on these rows, making the
        // "reconcile must also clear mtime" regression testable.
        db.update_file_mtime(id, 1_700_000_000_000_000_000 + i as i64)
            .unwrap();
    }
    db
}

fn all_hashes(db: &Database) -> Vec<String> {
    db.get_all_files()
        .unwrap()
        .into_iter()
        .map(|f| f.hash)
        .collect()
}

fn all_mtimes(db: &Database) -> Vec<i64> {
    db.get_all_files()
        .unwrap()
        .into_iter()
        .map(|f| f.mtime_nanos)
        .collect()
}

#[test]
fn parser_version_fresh_db_stamps_current_version() {
    let db = Database::open_in_memory().unwrap();
    // `Database::open_in_memory` auto-stamps on open, so the meta row
    // already exists by the time we inspect it. This test pins that
    // behaviour: after open, the stored version equals the current one
    // and the next reconcile observes `UpToDate`.
    assert_eq!(
        stored_parser_version(db.conn()).unwrap().as_deref(),
        Some(CURRENT_PARSER_VERSION)
    );
    let state = reconcile_parser_version(db.conn()).unwrap();
    assert!(matches!(state, ParserVersionState::UpToDate));
}

#[test]
fn parser_version_reports_fresh_on_raw_connection() {
    // Drive `reconcile_parser_version` directly against a bare connection
    // with only the meta table — this is the path `Database::open`'s
    // auto-stamp takes on a never-before-opened DB, before wrapping
    // into `Database`.
    use rusqlite::Connection;
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE files (
            id INTEGER PRIMARY KEY,
            path TEXT, hash TEXT, lang TEXT, size_bytes INTEGER
        );
        CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .unwrap();
    let state = reconcile_parser_version(&conn).unwrap();
    assert!(matches!(state, ParserVersionState::Fresh));
    assert_eq!(
        stored_parser_version(&conn).unwrap().as_deref(),
        Some(CURRENT_PARSER_VERSION)
    );
}

#[test]
fn parser_version_match_is_noop_on_hashes() {
    let db = make_indexed_db(&["h1", "h2", "h3"]);
    // Prime the DB with the current version.
    reconcile_parser_version(db.conn()).unwrap();

    // Second call: stored matches current → UpToDate, hashes preserved.
    let state = reconcile_parser_version(db.conn()).unwrap();
    assert!(matches!(state, ParserVersionState::UpToDate));
    let hashes = all_hashes(&db);
    assert_eq!(hashes, vec!["h1".to_string(), "h2".into(), "h3".into()]);
}

#[test]
fn parser_version_mismatch_clears_all_file_hashes() {
    let db = make_indexed_db(&["h1", "h2", "h3"]);
    // Simulate a DB written by an older rlm that stored "0.4.1".
    db.conn()
        .execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('parser_version', '0.4.1')",
            [],
        )
        .unwrap();

    let state = reconcile_parser_version(db.conn()).unwrap();
    match state {
        ParserVersionState::UpgradedFrom(prev) => assert_eq!(prev, "0.4.1"),
        other => panic!("expected UpgradedFrom, got {other:?}"),
    }

    let hashes = all_hashes(&db);
    assert_eq!(hashes, vec![String::new(); 3], "hashes should be cleared");
}

/// The staleness fast-path in `application::index::staleness` returns
/// early when `file.mtime_nanos == meta.mtime_nanos` — before it ever
/// looks at the hash. A parser-version upgrade must therefore clear
/// both fields, otherwise the next `rlm index` silently skips every
/// unchanged-on-disk file and the new parser's chunks never land.
#[test]
fn parser_version_mismatch_clears_mtime_to_bypass_staleness_fast_path() {
    let db = make_indexed_db(&["h1", "h2", "h3"]);
    // Seed a non-zero mtime per file — make_indexed_db already does
    // this, but pin the precondition explicitly so the test reads
    // self-contained.
    let before = all_mtimes(&db);
    assert!(
        before.iter().all(|&m| m > 0),
        "precondition: mtimes must be non-zero so the fast-path *would* skip them"
    );

    db.conn()
        .execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('parser_version', '0.4.1')",
            [],
        )
        .unwrap();

    let state = reconcile_parser_version(db.conn()).unwrap();
    assert!(matches!(state, ParserVersionState::UpgradedFrom(_)));

    let after = all_mtimes(&db);
    assert_eq!(
        after,
        vec![0; 3],
        "mtime_nanos must be reset to 0 so the staleness fast-path re-hashes instead of short-circuiting"
    );
}

#[test]
fn parser_version_mismatch_updates_stored_version() {
    let db = make_indexed_db(&["h1"]);
    db.conn()
        .execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('parser_version', '0.4.1')",
            [],
        )
        .unwrap();

    reconcile_parser_version(db.conn()).unwrap();
    assert_eq!(
        stored_parser_version(db.conn()).unwrap().as_deref(),
        Some(CURRENT_PARSER_VERSION)
    );
}

#[test]
fn parser_version_repeated_upgrade_reaches_uptodate() {
    let db = make_indexed_db(&["h1"]);
    db.conn()
        .execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('parser_version', '0.4.1')",
            [],
        )
        .unwrap();

    // First call: upgrades.
    assert!(matches!(
        reconcile_parser_version(db.conn()).unwrap(),
        ParserVersionState::UpgradedFrom(_)
    ));
    // Second call (same binary): stored already matches current.
    assert!(matches!(
        reconcile_parser_version(db.conn()).unwrap(),
        ParserVersionState::UpToDate
    ));
}
