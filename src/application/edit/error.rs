//! Re-export of [`EditError`].
//!
//! The type itself lives in `crate::error` so `RlmError` can wrap it via
//! `#[from]` without introducing a reverse dependency on `application::edit`.
//! This thin alias keeps `use super::error::EditError` working across the
//! edit subsystem.

pub use crate::error::EditError;
