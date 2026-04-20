//! Shared fixture for the `operations::savings` companion test files.
//!
//! Both `savings_tests` (legacy `record` API) and `savings_v2_tests`
//! (V2 `SavingsEntry` / scoped / symbol ops) need a fresh in-memory
//! database; centralising the constructor here keeps each split
//! companion free of duplicate-helper noise.

use crate::db::Database;

pub(super) fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}
