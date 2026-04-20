use crate::domain::chunk::{Chunk, ChunkKind};
use crate::error::Result;
use crate::ingest::text::TextParser;

/// Page-based PDF text extractor.
pub struct PdfParser;

impl Default for PdfParser {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl TextParser for PdfParser {
    fn format(&self) -> &'static str {
        "pdf"
    }

    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let mut chunks = Vec::new();

        // Split by form feed (page break) or double newlines as page separators
        let pages: Vec<&str> = if source.contains('\x0C') {
            source.split('\x0C').collect()
        } else {
            // Fallback: treat entire content as one page
            vec![source]
        };

        let mut byte_offset = 0u64;
        let mut line_offset = 1u32;

        for (i, page_content) in pages.iter().enumerate() {
            let trimmed = page_content.trim();
            if trimmed.is_empty() {
                byte_offset += page_content.len() as u64 + 1; // +1 for page break char
                continue;
            }

            // If byte_offset exceeds u32::MAX, skip remaining pages to avoid truncation
            if byte_offset > u64::from(u32::MAX) {
                break;
            }

            let line_count = page_content.lines().count() as u32;
            let page_num = i + 1;
            let start_byte = byte_offset as u32;
            let page_len = page_content.len() as u64;
            let end_byte_u64 = byte_offset + page_len;
            if end_byte_u64 > u64::from(u32::MAX) {
                break; // Skip this and remaining pages — byte range would be truncated
            }
            let end_byte = end_byte_u64 as u32;

            chunks.push(Chunk {
                start_line: line_offset,
                end_line: line_offset + line_count.saturating_sub(1),
                start_byte,
                end_byte,
                kind: ChunkKind::Page,
                ident: format!("Page {page_num}"),
                content: trimmed.to_string(),
                ..Chunk::stub(file_id)
            });

            line_offset += line_count;
            byte_offset += page_len + 1;
        }

        Ok(chunks)
    }
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
