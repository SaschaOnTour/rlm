//! Shared fixture for the `interface::shared::savings_middleware`
//! companion test files (`savings_middleware_tests` +
//! `savings_middleware_scoped_tests`).
//!
//! Centralising `test_db` and the tiny `Payload` DTO here keeps the two
//! split companions free of duplicate-helper / dead-code warnings.

use crate::db::Database;
use serde::Serialize;

pub(crate) fn test_db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

#[derive(Serialize)]
pub(crate) struct Payload {
    pub(crate) label: String,
}

pub(crate) fn payload(label: &str) -> Payload {
    Payload {
        label: label.into(),
    }
}
