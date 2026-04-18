pub mod partition;
pub mod summarize;

// Slice 3.2 moved `rlm::peek` to `crate::application::query::peek`.
// Re-export here so adapters still importing through the old path
// keep compiling; later slices update those imports.
pub use crate::application::query::peek;
