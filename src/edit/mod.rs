// Slice 3.4 moved the contents of this module to
// `crate::application::edit::*` and renamed `syntax_guard` to
// `validator`. Re-exports keep the old paths compilable until adapters
// migrate.
pub use crate::application::edit::validator as syntax_guard;
pub use crate::application::edit::{error, inserter, replacer};
