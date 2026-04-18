//! `FileQuery` trait for path-keyed, single-file read-only queries.
//!
//! Every implementor answers the same shape: given a db and a project-
//! relative path, produce a serializable result. The caller does not
//! need to know what command it is — the savings middleware reads the
//! command name off the trait and records the entry with the
//! `SingleFile` cost model. Implementors carry their own extra state
//! (e.g. `ScopeQuery::line`, `PartitionQuery::strategy`) in struct
//! fields; `execute(&self, db, path)` accesses it via `self`.
//!
//! Contrast with `application::symbol::SymbolQuery`, which uses the
//! `SymbolFiles` cost model and keys by symbol identifier instead of
//! path. Both traits coexist under `application::*` because both are
//! use-case contracts — only the domain shape differs.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

/// A read-only query that operates on a single file, keyed by its
/// project-relative path. `files_touched` is always 1 — that is what
/// makes it a `FileQuery` — so the trait does not need a separate
/// method for it.
pub trait FileQuery {
    /// Typed result produced by this query.
    type Output: Serialize;

    /// Command name recorded in the savings table (must be a stable
    /// identifier; the savings report groups by it).
    const COMMAND: &'static str;

    /// Run the query against the index. `self` carries any extra
    /// parameters the query needs beyond `path` (e.g. line number for
    /// `ScopeQuery`, strategy for `PartitionQuery`).
    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output>;
}
