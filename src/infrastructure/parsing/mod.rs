//! Shared tree-sitter infrastructure: cursor traversal and query
//! execution helpers used by every language parser.
//!
//! Introduced by slice 4.2 to pull the generic tree-walking logic out
//! of `ingest::code::helpers` (which keeps only string-extraction
//! utilities). Slice 4.3 and onward migrate the per-language parsers
//! onto these helpers.

pub mod query_runner;
pub mod tree_walker;
