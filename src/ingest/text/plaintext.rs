use crate::domain::chunk::{Chunk, ChunkKind};
use crate::error::Result;
use crate::ingest::text::TextParser;

/// Minimal text parser that treats the entire file as a single chunk.
/// Provides FTS5 searchability for file types without dedicated parsers.
pub struct PlaintextParser;

impl Default for PlaintextParser {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaintextParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl TextParser for PlaintextParser {
    fn format(&self) -> &'static str {
        "plaintext"
    }

    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        if source.trim().is_empty() {
            return Ok(Vec::new());
        }

        let lines: Vec<&str> = source.lines().collect();

        Ok(vec![Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: lines.len() as u32,
            start_byte: 0,
            end_byte: source.len() as u32,
            kind: ChunkKind::Section,
            ident: "(document)".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: source.to_string(),
        }])
    }
}

#[cfg(test)]
#[path = "plaintext_tests.rs"]
mod tests;
