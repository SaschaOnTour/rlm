//! Tests for `refs.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "refs_tests.rs"] mod tests;`.

use super::super::impact::{ImpactEntry, ImpactResult};
use super::super::SymbolQuery;
use super::RefsQuery;

#[test]
fn refs_query_command_is_stable() {
    assert_eq!(RefsQuery::COMMAND, "refs");
}

#[test]
fn refs_query_delegates_file_count_to_impact_result() {
    let result = ImpactResult {
        symbol: "foo".into(),
        impacted: vec![
            ImpactEntry {
                file: "src/a.rs".into(),
                in_symbol: "caller".into(),
                line: 10,
                ref_kind: "call".into(),
            },
            ImpactEntry {
                file: "src/a.rs".into(),
                in_symbol: "other".into(),
                line: 20,
                ref_kind: "call".into(),
            },
        ],
        count: 2,
        tokens: Default::default(),
    };
    assert_eq!(RefsQuery::file_count(&result), 1);
}
