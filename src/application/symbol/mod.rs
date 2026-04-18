//! Symbol-scoped analyses and the `SymbolQuery` trait they share.
//!
//! Every analysis in this module follows the same shape — take a
//! symbol identifier, consult the index, return a typed result whose
//! distinct-file count feeds the savings middleware's `SymbolFiles`
//! cost model. The trait captures that contract so the adapters can
//! treat every symbol query uniformly via
//! `interface::shared::record_symbol_query`.
//!
//! `scope` lives here for the domain grouping (all symbol-related
//! queries) but is keyed by `path + line` and uses the `SingleFile`
//! cost model; slice 3.6b wires it to a `FileQuery` trait instead of
//! `SymbolQuery`.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

pub mod callgraph;
pub mod context;
pub mod impact;
pub mod refs;
pub mod scope;
pub mod signature;
pub mod type_info;

pub use context::{ContextQuery, ContextWithGraphQuery};
pub use refs::RefsQuery;
pub use scope::ScopeQuery;

/// A read-only analysis of a symbol.
///
/// Implementors are usually zero-sized marker types (e.g. `RefsQuery`)
/// whose associated items describe the operation: the result type, the
/// command identifier for savings recording, how to execute the
/// analysis, and how to count distinct files in the result.
pub trait SymbolQuery {
    /// Typed result produced by this query.
    type Output: Serialize;

    /// Command name recorded in the savings table (must be a stable
    /// identifier; the savings report groups by it).
    const COMMAND: &'static str;

    /// Run the analysis against the index.
    fn execute(db: &Database, symbol: &str) -> Result<Self::Output>;

    /// Distinct source files involved in the result. Used by the
    /// savings middleware as `OperationMeta::files_touched` for the
    /// `SymbolFiles` cost model.
    fn file_count(output: &Self::Output) -> u64;
}
