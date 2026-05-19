//! Write operations (insert, replace) and the syntax validator that
//! gates every write.
//!
//! Slice 3.4 moved these in from `crate::edit::*` and renamed the
//! `syntax_guard` module to `validator`. The `SyntaxGuard` struct name
//! and `validate_and_write` helper are unchanged — only the module
//! name changed to match the "validator in front of writer" semantics.

pub mod error;
pub mod extractor;
pub mod inserter;
pub mod native_check;
pub mod replacer;
pub mod savings_hooks;
pub mod validator;
pub mod write_dispatch;

// Re-exports: the request DTOs `ReplaceInput` and `DeleteInput` are
// owned by `write_dispatch` but flow through `replacer` as well — the
// low-level primitives (`replace_symbol`, `delete_symbol`) take them
// by reference so the dispatcher and the primitive share one shape.
// Promoting them to the `edit::` namespace keeps the imports symmetric
// from both sides without forcing `replacer.rs` to reach sideways into
// its sibling `write_dispatch`.
pub use write_dispatch::{DeleteInput, ReplaceInput};
