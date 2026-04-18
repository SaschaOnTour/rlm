// Slice 3.2 and 3.3 moved the contents of this module to
// `crate::application::query::*` and `crate::application::content::*`.
// Re-exports keep the old paths compilable until adapters migrate.
pub use crate::application::content::{partition, summarize};
pub use crate::application::query::peek;
