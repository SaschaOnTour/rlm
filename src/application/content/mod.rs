//! Content transformations (partition, summarize, deps, diff).
//!
//! Slice 3.3 moved these in from `crate::rlm::*` (partition, summarize)
//! and `crate::operations::*` (deps, diff). The legacy paths still
//! re-export them for adapters that have not been migrated yet.

pub mod deps;
pub mod diff;
pub mod partition;
pub mod summarize;

pub use deps::DepsQuery;
pub use diff::{DiffFileQuery, DiffSymbolQuery};
pub use partition::PartitionQuery;
pub use summarize::SummarizeQuery;
