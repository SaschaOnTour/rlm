//! Shared fixtures for the `application::symbol` companion test files.
//!
//! Each split companion (`impact_tests`, `impact_ref_kind_tests`,
//! `callgraph_tests`, `callgraph_refs_tests`, `context_tests`,
//! `context_graph_tests`) needs an in-memory database. Centralising
//! `setup_test_db` here keeps the fixture out of the duplicate detector
//! and the dead-code / unwrap-in-tests noise from triggering per file.

use crate::db::Database;

pub(crate) fn setup_test_db() -> Database {
    Database::open_in_memory().unwrap()
}
