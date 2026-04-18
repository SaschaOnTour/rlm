//! Chunk entity — a coherent unit of code or document content.

use serde::{Deserialize, Serialize};

use super::{ChunkId, FileId};

/// A byte range within a source file, half-open: `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

impl ByteRange {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// A line range within a source file. 1-based, inclusive on both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn line_count(&self) -> u32 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }
}

/// The kind of a code or document chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Interface,
    Module,
    Constant,
    Section,
    Page,
    Other(String),
}

impl ChunkKind {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Function => "fn",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Module => "mod",
            Self::Constant => "const",
            Self::Section => "section",
            Self::Page => "page",
            Self::Other(s) => s.as_str(),
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "fn" => Self::Function,
            "method" => Self::Method,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "trait" => Self::Trait,
            "impl" => Self::Impl,
            "class" => Self::Class,
            "interface" => Self::Interface,
            "mod" => Self::Module,
            "const" => Self::Constant,
            "section" => Self::Section,
            "page" => Self::Page,
            other => Self::Other(other.to_string()),
        }
    }

    #[must_use]
    pub fn is_section(&self) -> bool {
        matches!(self, Self::Section | Self::Page)
    }
}

/// A chunk of code or document content produced by the indexer.
///
/// Positional information is carried by the `bytes` and `lines` value
/// objects rather than as four loose `u32` fields, which matches how chunks
/// are naturally reasoned about (a region of a file) and avoids argument
/// order mistakes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: ChunkId,
    pub file_id: FileId,
    pub bytes: ByteRange,
    pub lines: LineRange,
    pub kind: ChunkKind,
    pub ident: String,
    pub parent: Option<String>,
    pub signature: Option<String>,
    pub visibility: Option<String>,
    pub ui_ctx: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<String>,
    pub content: String,
}

impl Chunk {
    #[must_use]
    pub fn line_count(&self) -> u32 {
        self.lines.line_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_range_len_and_empty() {
        let r = ByteRange::new(10, 25);
        assert_eq!(r.len(), 15);
        assert!(!r.is_empty());

        let empty = ByteRange::new(5, 5);
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());

        let inverted = ByteRange::new(10, 3);
        assert_eq!(inverted.len(), 0);
        assert!(inverted.is_empty());
    }

    #[test]
    fn line_range_counts_inclusive() {
        assert_eq!(LineRange::new(5, 5).line_count(), 1);
        assert_eq!(LineRange::new(1, 10).line_count(), 10);
        assert_eq!(LineRange::new(42, 50).line_count(), 9);
    }

    #[test]
    fn chunk_kind_round_trip() {
        let kinds = [
            ChunkKind::Function,
            ChunkKind::Method,
            ChunkKind::Struct,
            ChunkKind::Enum,
            ChunkKind::Trait,
            ChunkKind::Impl,
            ChunkKind::Class,
            ChunkKind::Interface,
            ChunkKind::Module,
            ChunkKind::Constant,
            ChunkKind::Section,
            ChunkKind::Page,
        ];
        for k in kinds {
            assert_eq!(ChunkKind::parse(k.as_str()), k);
        }
    }

    #[test]
    fn chunk_kind_other_round_trip() {
        let k = ChunkKind::Other("namespace".into());
        assert_eq!(k.as_str(), "namespace");
        assert_eq!(ChunkKind::parse("namespace"), k);
    }

    #[test]
    fn chunk_kind_is_section_only_for_document_kinds() {
        assert!(ChunkKind::Section.is_section());
        assert!(ChunkKind::Page.is_section());
        assert!(!ChunkKind::Function.is_section());
        assert!(!ChunkKind::Class.is_section());
        assert!(!ChunkKind::Other("tag".into()).is_section());
    }

    #[test]
    fn chunk_line_count_delegates_to_range() {
        let c = sample_chunk(LineRange::new(5, 15));
        assert_eq!(c.line_count(), 11);
    }

    fn sample_chunk(lines: LineRange) -> Chunk {
        Chunk {
            id: ChunkId::UNPERSISTED,
            file_id: FileId::new(1),
            bytes: ByteRange::new(0, 100),
            lines,
            kind: ChunkKind::Function,
            ident: "foo".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: String::new(),
        }
    }
}
