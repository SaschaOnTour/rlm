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

    /// Parse markdown into heading-based chunks (integration: calls only, no logic).
    ///
    /// Delegates heading discovery to `collect_heading_positions` and
    /// per-section chunk building to `build_section_chunk`.
    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let lines: Vec<&str> = source.lines().collect();
        let sections = collect_heading_positions(&lines);

        if sections.is_empty() {
            return Ok(build_document_fallback(source, &lines, file_id));
        }

        let ctx = SectionBuildContext {
            source,
            lines: &lines,
            sections: &sections,
            file_id,
        };
        let chunks = sections
            .iter()
            .enumerate()
            .map(|(idx, section)| build_section_chunk(&ctx, idx, section))
            .collect();

        Ok(chunks)
    }
}

struct SectionStart {
    line: usize,
    level: usize,
    heading: String,
}

/// Context bundle for building section chunks, reducing parameter count.
struct SectionBuildContext<'a> {
    source: &'a str,
    lines: &'a [&'a str],
    sections: &'a [SectionStart],
    file_id: i64,
}

/// Scan lines for markdown headings, returning their positions (operation: logic only).
fn collect_heading_positions(lines: &[&str]) -> Vec<SectionStart> {
    let mut sections = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            continue;
        }
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
    sections
}

/// Build a single fallback chunk when no headings are found (operation: logic only).
fn build_document_fallback(source: &str, lines: &[&str], file_id: i64) -> Vec<Chunk> {
    if source.trim().is_empty() {
        return Vec::new();
    }
    vec![Chunk {
        start_line: 1,
        end_line: lines.len() as u32,
        end_byte: source.len() as u32,
        kind: ChunkKind::Section,
        ident: "(document)".into(),
        content: source.to_string(),
        ..Chunk::stub(file_id)
    }]
}

/// Build a `Chunk` for one heading section (operation: logic only).
///
/// Computes line range, byte offsets, content, and parent heading
/// from the section list context.
fn build_section_chunk(
    ctx: &SectionBuildContext<'_>,
    idx: usize,
    section: &SectionStart,
) -> Chunk {
    let start_line = section.line;
    let end_line = if idx + 1 < ctx.sections.len() {
        ctx.sections[idx + 1].line.saturating_sub(1)
    } else {
        ctx.lines.len() - 1
    };

    let content = ctx.lines[start_line..=end_line].join("\n");
    let start_byte = byte_offset_of_line(ctx.source, start_line);
    let end_byte = if end_line + 1 < ctx.lines.len() {
        byte_offset_of_line(ctx.source, end_line + 1)
    } else {
        ctx.source.len()
    };

    let parent = ctx.sections[..idx]
        .iter()
        .rev()
        .find(|s| s.level < section.level)
        .map(|s| s.heading.clone());

    Chunk {
        start_line: start_line as u32 + 1,
        end_line: end_line as u32 + 1,
        start_byte: start_byte as u32,
        end_byte: end_byte as u32,
        kind: ChunkKind::Section,
        ident: section.heading.clone(),
        parent,
        content,
        ..Chunk::stub(ctx.file_id)
    }
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
