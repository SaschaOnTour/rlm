//! MCP tool-handler facade.
//!
//! The previous single-file module grew past the SRP-module threshold. The
//! handlers are now split by concern across four sibling modules and
//! re-exported here so callers (`server.rs`, tests) keep using
//! `tool_handlers::handle_*` unchanged:
//!
//! - `tool_handlers_index` — `handle_index` / `handle_index_with_progress`
//! - `tool_handlers_query` — `handle_search` / `handle_overview` / `handle_refs` / `handle_files`
//! - `tool_handlers_read`  — `handle_read` (symbol + section dispatch)
//! - `tool_handlers_edit`  — `handle_replace` / `handle_insert` + `InsertInput`
//!
//! Utility handlers (savings, verify, …) still live in `tool_handlers_util`.

pub use super::tool_handlers_edit::{handle_insert, handle_replace, InsertInput};
pub use super::tool_handlers_index::{handle_index, handle_index_with_progress};
pub use super::tool_handlers_query::{handle_files, handle_overview, handle_refs, handle_search};
pub use super::tool_handlers_read::handle_read;

#[cfg(test)]
#[path = "tool_handlers_tests.rs"]
mod tests;
