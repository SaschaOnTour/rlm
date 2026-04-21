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
        db.upsert_file(&f).unwrap();
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
