//! Content transformations (partition, summarize, deps, diff).

pub mod deps;
pub mod diff;
pub mod partition;
pub mod summarize;

pub use deps::DepsQuery;
pub use diff::{DiffFileQuery, DiffSymbolQuery};
pub use partition::PartitionQuery;
pub use summarize::SummarizeQuery;

#[cfg(test)]
#[path = "fixtures_tests.rs"]
mod fixtures;
