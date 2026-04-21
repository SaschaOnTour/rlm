//! Domain entities.
//!
//! Consolidated in slice 6.2: `Chunk` and `FileRecord` (the working
//! types the indexer writes and queries return) live here along with
//! the token-budget math and savings formulas. Nothing in this
//! module depends on `rusqlite`, `tree_sitter`, or the filesystem.

pub mod chunk;
pub mod file;
pub mod savings;
pub mod token_budget;

pub use chunk::{Chunk, ChunkKind, RefKind, Reference};
pub use file::FileRecord;
pub use savings::{CommandSavings, SavingsEntry, SavingsReport};
pub use token_budget::TokenEstimate;
