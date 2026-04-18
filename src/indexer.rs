// Slice 3.8 moved the contents of this module to
// `crate::application::index::*`. Re-exports keep the old paths
// compilable until adapters migrate.
pub use crate::application::index::staleness;
pub use crate::application::index::*;
