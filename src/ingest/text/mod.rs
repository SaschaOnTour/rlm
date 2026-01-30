pub mod json_semantic;
pub mod markdown;
pub mod pdf;
pub mod plaintext;
pub mod toml_parser;
pub mod yaml;

use crate::error::Result;
use crate::models::chunk::Chunk;

/// Trait for structure-aware text parsers (non-code).
pub trait TextParser: Send + Sync {
    /// Language/format identifier.
    fn format(&self) -> &str;

    /// Parse text content and extract structured chunks (sections, pages).
    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>>;
}
