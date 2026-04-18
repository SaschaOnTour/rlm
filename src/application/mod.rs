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
//! This module is the skeleton introduced by slice 3.1. Later slices
//! migrate existing code from `crate::operations`, `crate::rlm`,
//! `crate::search`, `crate::edit`, and `crate::indexer` into the
//! corresponding sub-domains.

pub mod content;
pub mod edit;
pub mod index;
pub mod query;
pub mod symbol;
