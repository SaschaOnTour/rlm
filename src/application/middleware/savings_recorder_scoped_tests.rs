//! Scoped / symbol-files middleware tests for `savings_middleware.rs`.
//!
//! Split out of `savings_middleware_tests.rs` to keep each companion
//! focused on a smaller cluster of behaviors (SRP_MODULE). Single-file
//! and fixed/at-least-body variants stay in `savings_middleware_tests.rs`;
//! this file covers the scoped-prefix and symbol-files alternatives.

use super::super::fixtures::{payload, test_db};
use super::{record_operation, AlternativeCost, OperationMeta};

#[test]
fn records_symbol_files_op() {
    let db = test_db();
    let meta = OperationMeta {
        command: "refs",
        files_touched: 3,
        alternative: AlternativeCost::SymbolFiles {
            symbol: "foo".into(),
        },
    };

    let response = record_operation(&db, &meta, &payload("refs-out"));
    assert!(response.body.contains("refs-out"));

    let rows = db.get_savings_by_command(None).unwrap();
    assert!(rows.iter().any(|r| r.command == "refs"));
}

#[test]
fn records_scoped_op_with_prefix() {
    let db = test_db();
    let meta = OperationMeta {
        command: "overview",
        files_touched: 0,
        alternative: AlternativeCost::ScopedFiles {
            prefix: Some("src/".into()),
        },
    };

    let response = record_operation(&db, &meta, &payload("o"));
    assert!(response.body.contains('o'));

    let rows = db.get_savings_by_command(None).unwrap();
    assert!(rows.iter().any(|r| r.command == "overview"));
}
