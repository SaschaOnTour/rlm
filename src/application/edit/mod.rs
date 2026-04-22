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
