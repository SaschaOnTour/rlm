//! Query use cases — read-only retrievals across the indexed project.

pub mod files;
pub mod map;
pub mod peek;
pub mod read;
pub mod search;
pub mod stats;
pub mod supported;
pub mod tree;
pub mod verify;

#[cfg(test)]
#[path = "fixtures_tests.rs"]
mod fixtures;
