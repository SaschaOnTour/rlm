//! Application layer — use cases orchestrating domain entities and
//! infrastructure abstractions.
//!
//! Sub-domains:
//!
//! - [`query`] — read-only retrievals (peek, grep, search, map, tree,
//!   read, stats, files, verify, supported).
//! - [`symbol`] — symbol-scoped analyses (refs, signature, callgraph,
//!   impact, context, type_info, scope). Slice 3.5 introduces the
//!   `SymbolQuery` trait they share.
//! - [`content`] — content transformations (partition, summarize, deps,
//!   diff).
//! - [`edit`] — write operations (insert, replace) plus the syntax
//!   validator gating them.
//! - [`index`] — the indexing pipeline (scan → parse → insert stages).
//!
//! Every use case in this layer depends downward on [`crate::domain`]
//! (pure entities) and [`crate::infrastructure`] (via repository traits),
//! never on the concrete backends. Adapters in [`crate::interface`] call
//! into here.
//!
//! This module started as the skeleton introduced by slice 3.1. Phases
//! 3–5 then migrated existing code from the former top-level
//! `crate::operations`, `crate::rlm`, `crate::search`, `crate::edit`,
//! and `crate::indexer` modules into the sub-domains here. Those
//! bridges have been removed; `crate::operations` is the one remaining
//! top-level module of its vintage and still holds its own code
//! (refs, savings, index output envelope).

pub mod content;
pub mod dto;
pub mod edit;
pub mod file_query;
pub mod index;
pub mod query;
pub mod symbol;

pub use file_query::FileQuery;
