//! Shared fixtures for the `application::query` companion test files.
//!
//! `setup_test_db` is used by `map_tests` / `map_advanced_tests`;
//! `setup_test_db_and_dir` by `verify_tests` / `verify_fix_tests`.
//! Centralising them here keeps each split companion off the
//! duplicate-helper warning list.

use crate::db::Database;
use tempfile::TempDir;

pub(crate) fn setup_test_db() -> Database {
    Database::open_in_memory().unwrap()
}

pub(crate) fn setup_test_db_and_dir() -> (Database, TempDir) {
    let db = Database::open_in_memory().unwrap();
    let tmp = TempDir::new().unwrap();
    (db, tmp)
}
