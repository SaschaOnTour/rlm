//! Pure domain entities.
//!
//! This module holds the entities rlm reasons about — chunks, files,
//! references — without any persistence or I/O concerns. Nothing in this
//! module depends on `rusqlite`, `tree_sitter`, or the filesystem.

pub mod chunk;
pub mod file;
pub mod ids;
pub mod reference;
pub mod savings;
pub mod token_budget;

pub use chunk::{ByteRange, Chunk, ChunkKind, LineRange};
pub use file::File;
pub use ids::{ChunkId, FileId, ReferenceId};
pub use reference::{RefKind, Reference};
pub use savings::{CommandSavings, SavingsEntry, SavingsReport};
pub use token_budget::TokenEstimate;
