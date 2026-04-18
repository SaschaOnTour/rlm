//! Query use cases — read-only retrievals across the indexed project.
//!
//! Slice 3.2 migrated these from `crate::operations::*`,
//! `crate::rlm::peek`, and `crate::search::tree` into one home. The
//! legacy paths still re-export for adapters that have not yet been
//! migrated.

pub mod files;
pub mod map;
pub mod peek;
pub mod search;
pub mod stats;
pub mod supported;
pub mod tree;
pub mod verify;
