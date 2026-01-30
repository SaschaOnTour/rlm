use crate::error::{Result, RlmError};
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

    /// Extract text from a PDF file path.
    pub fn extract_from_file(&self, path: &std::path::Path, file_id: i64) -> Result<Vec<Chunk>> {
        let bytes = std::fs::read(path)?;
        let text = pdf_extract::extract_text_from_mem(&bytes).map_err(|e| RlmError::Parse {
            path: path.to_string_lossy().into(),
            detail: format!("PDF extraction error: {e}"),
        })?;
        self.parse_chunks(&text, file_id)
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

        let mut byte_offset = 0u32;
        let mut line_offset = 1u32;

        for (i, page_content) in pages.iter().enumerate() {
            let trimmed = page_content.trim();
            if trimmed.is_empty() {
                byte_offset += page_content.len() as u32 + 1; // +1 for page break char
                continue;
            }

            let line_count = page_content.lines().count() as u32;
            let page_num = i + 1;

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line: line_offset,
                end_line: line_offset + line_count.saturating_sub(1),
                start_byte: byte_offset,
                end_byte: byte_offset + page_content.len() as u32,
                kind: ChunkKind::Page,
                ident: format!("Page {page_num}"),
                parent: None,
                signature: None,
                visibility: None,
                ui_ctx: None,
                doc_comment: None,
                attributes: None,
                content: trimmed.to_string(),
            });

            line_offset += line_count;
            byte_offset += page_content.len() as u32 + 1;
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
