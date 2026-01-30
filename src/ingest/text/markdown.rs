use crate::error::Result;
use crate::ingest::text::TextParser;
use crate::models::chunk::{Chunk, ChunkKind};

/// Heading-based markdown section parser.
pub struct MarkdownParser;

impl Default for MarkdownParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl TextParser for MarkdownParser {
    fn format(&self) -> &'static str {
        "markdown"
    }

    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let lines: Vec<&str> = source.lines().collect();
        let mut chunks = Vec::new();
        let mut sections: Vec<SectionStart> = Vec::new();

        // Find all heading positions
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let level = trimmed.chars().take_while(|c| *c == '#').count();
                let heading = trimmed[level..].trim().to_string();
                if !heading.is_empty() {
                    sections.push(SectionStart {
                        line: i,
                        level,
                        heading,
                    });
                }
            }
        }

        if sections.is_empty() {
            // No headings: treat entire file as one chunk
            if !source.trim().is_empty() {
                chunks.push(Chunk {
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
                });
            }
            return Ok(chunks);
        }

        // Build chunks from sections
        for (idx, section) in sections.iter().enumerate() {
            let start_line = section.line;
            let end_line = if idx + 1 < sections.len() {
                sections[idx + 1].line.saturating_sub(1)
            } else {
                lines.len() - 1
            };

            let content = lines[start_line..=end_line].join("\n");
            let start_byte = byte_offset_of_line(source, start_line);
            let end_byte = if end_line + 1 < lines.len() {
                byte_offset_of_line(source, end_line + 1)
            } else {
                source.len()
            };

            // Find parent heading (nearest heading with lower level)
            let parent = sections[..idx]
                .iter()
                .rev()
                .find(|s| s.level < section.level)
                .map(|s| s.heading.clone());

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line: start_line as u32 + 1,
                end_line: end_line as u32 + 1,
                start_byte: start_byte as u32,
                end_byte: end_byte as u32,
                kind: ChunkKind::Section,
                ident: section.heading.clone(),
                parent,
                signature: None,
                visibility: None,
                ui_ctx: None,
                doc_comment: None,
                attributes: None,
                content,
            });
        }

        Ok(chunks)
    }
}

struct SectionStart {
    line: usize,
    level: usize,
    heading: String,
}

fn byte_offset_of_line(source: &str, line_idx: usize) -> usize {
    source
        .lines()
        .take(line_idx)
        .map(|l| l.len() + 1) // +1 for newline
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> MarkdownParser {
        MarkdownParser::new()
    }

    #[test]
    fn parse_headings() {
        let source =
            "# Title\n\nIntro text\n\n## Section 1\n\nContent 1\n\n## Section 2\n\nContent 2\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].ident, "Title");
        assert_eq!(chunks[1].ident, "Section 1");
        assert_eq!(chunks[1].parent.as_deref(), Some("Title"));
        assert_eq!(chunks[2].ident, "Section 2");
    }

    #[test]
    fn parse_nested_headings() {
        let source = "# Top\n\n## Sub\n\n### Sub-sub\n\nContent\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[2].ident, "Sub-sub");
        assert_eq!(chunks[2].parent.as_deref(), Some("Sub"));
    }

    #[test]
    fn parse_no_headings() {
        let source = "Just some text\nwithout any headings\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ident, "(document)");
    }

    #[test]
    fn parse_empty() {
        let chunks = parser().parse_chunks("", 1).unwrap();
        assert!(chunks.is_empty());
    }
}
