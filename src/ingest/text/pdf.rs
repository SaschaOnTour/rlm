use crate::error::Result;
use crate::ingest::text::TextParser;
use crate::models::chunk::{Chunk, ChunkKind};

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
            let end_byte = if end_byte_u64 > u64::from(u32::MAX) {
                u32::MAX
            } else {
                end_byte_u64 as u32
            };

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
mod tests {
    use super::*;

    fn parser() -> PdfParser {
        PdfParser::new()
    }

    #[test]
    fn parse_single_page() {
        let source = "This is page one content.\nSecond line.";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ident, "Page 1");
        assert_eq!(chunks[0].kind, ChunkKind::Page);
    }

    #[test]
    fn parse_multiple_pages() {
        let source = "Page one content\x0CPage two content\x0CPage three content";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].ident, "Page 1");
        assert_eq!(chunks[1].ident, "Page 2");
        assert_eq!(chunks[2].ident, "Page 3");
    }

    #[test]
    fn parse_empty_pages_skipped() {
        let source = "Content\x0C\x0CMore content";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 2);
    }
}
