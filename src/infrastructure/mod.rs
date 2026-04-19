//! Infrastructure — adapters to external systems.
//!
//! Concrete implementations of domain-level interfaces live here: database
//! access, parser backends, filesystem I/O. Upper layers (`application/`,
//! `interface/`) depend on these abstractions, never on the concrete
//! crates (`rusqlite`, `tree_sitter`, `ignore`).

pub mod filesystem;
pub mod parsing;
pub mod persistence;
