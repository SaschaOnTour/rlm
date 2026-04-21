//! Tests for `request.rs`.
//!
//! Moved to a companion file so the `panic!("unexpected variant")`
//! match guards used as test assertions stay clear of the
//! `no_panic_macros_in_production` rule on prod code. Wired back in
//! via `#[cfg(test)] #[path = "request_tests.rs"] mod tests;`.

use super::{AlternativeCost, OperationMeta};

#[test]
fn meta_holds_all_fields() {
    let m = OperationMeta {
        command: "search",
        files_touched: 3,
        alternative: AlternativeCost::Fixed(1_000),
    };
    assert_eq!(m.command, "search");
    assert_eq!(m.files_touched, 3);
    assert!(matches!(m.alternative, AlternativeCost::Fixed(1_000)));
}

// One test per variant. Each constructs an `OperationMeta` around
// the variant under test — the struct construction is what rustqual's
// SUT detector recognises as exercising production code, and it
// mirrors how callers actually wire up an `AlternativeCost` in the
// pipeline.

#[test]
fn alternative_cost_single_file_carries_path() {
    let m = OperationMeta {
        command: "read",
        files_touched: 1,
        alternative: AlternativeCost::SingleFile {
            path: "src/main.rs".into(),
        },
    };
    assert!(matches!(
        &m.alternative,
        AlternativeCost::SingleFile { path } if path == "src/main.rs"
    ));
}

#[test]
fn alternative_cost_symbol_files_carries_symbol() {
    let m = OperationMeta {
        command: "refs",
        files_touched: 0,
        alternative: AlternativeCost::SymbolFiles {
            symbol: "foo".into(),
        },
    };
    assert!(matches!(
        &m.alternative,
        AlternativeCost::SymbolFiles { symbol } if symbol == "foo"
    ));
}

#[test]
fn alternative_cost_scoped_files_with_prefix_keeps_prefix() {
    let m = OperationMeta {
        command: "overview",
        files_touched: 0,
        alternative: AlternativeCost::ScopedFiles {
            prefix: Some("src/".into()),
        },
    };
    assert!(matches!(
        &m.alternative,
        AlternativeCost::ScopedFiles { prefix: Some(p) } if p == "src/"
    ));
}

#[test]
fn alternative_cost_scoped_files_without_prefix_covers_whole_project() {
    let m = OperationMeta {
        command: "overview",
        files_touched: 0,
        alternative: AlternativeCost::ScopedFiles { prefix: None },
    };
    assert!(matches!(
        &m.alternative,
        AlternativeCost::ScopedFiles { prefix: None }
    ));
}

#[test]
fn alternative_cost_at_least_body_carries_base() {
    let m = OperationMeta {
        command: "search",
        files_touched: 0,
        alternative: AlternativeCost::AtLeastBody { base: 42 },
    };
    assert!(matches!(
        &m.alternative,
        AlternativeCost::AtLeastBody { base: 42 }
    ));
}
