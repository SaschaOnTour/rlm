//! `refs` / impact analysis as a [`SymbolQuery`].
//!
//! Wraps `analyze_impact` in the `SymbolQuery` trait so adapters can
//! dispatch through `record_symbol_query::<RefsQuery>`.

use crate::db::Database;
use crate::error::Result;

use super::impact::{analyze_impact, ImpactResult};
use super::SymbolQuery;

/// Find all usages of a symbol and report distinct affected files.
pub struct RefsQuery;

impl SymbolQuery for RefsQuery {
    type Output = ImpactResult;
    const COMMAND: &'static str = "refs";

    fn execute(db: &Database, symbol: &str) -> Result<Self::Output> {
        analyze_impact(db, symbol)
    }

    fn file_count(output: &Self::Output) -> u64 {
        output.file_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_query_command_is_stable() {
        assert_eq!(RefsQuery::COMMAND, "refs");
    }

    #[test]
    fn refs_query_delegates_file_count_to_impact_result() {
        use super::super::impact::ImpactEntry;

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
}
