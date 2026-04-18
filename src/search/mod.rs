pub mod fts;

// Slice 3.2 moved `search::tree` to `crate::application::query::tree`.
// Re-export here so adapters still importing through the old path
// keep compiling; later slices update those imports.
pub use crate::application::query::tree;
