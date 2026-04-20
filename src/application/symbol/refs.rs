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
#[path = "refs_tests.rs"]
mod tests;
