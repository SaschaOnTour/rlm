//! Shared fixture for the `application::content` companion test files.
//!
//! Used by `diff_tests` / `diff_symbol_tests` to share the DB + TempDir
//! setup so each split companion stays free of duplicate helpers.

use crate::db::Database;
use tempfile::TempDir;

pub(crate) fn setup_test_db_and_dir() -> (Database, TempDir) {
    let db = Database::open_in_memory().unwrap();
    let tmp = TempDir::new().unwrap();
    (db, tmp)
}
