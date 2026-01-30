use serde::Serialize;

/// The kind of a code/document chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    Section, // markdown heading
    Page,    // PDF page
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
}

/// A chunk of code or document content extracted during indexing.
#[derive(Debug, Clone, Serialize)]
pub struct Chunk {
    /// Database row ID (0 if not yet persisted).
    pub id: i64,
    /// Foreign key to the file record.
    pub file_id: i64,
    /// Start line (1-based).
    pub start_line: u32,
    /// End line (1-based, inclusive).
    pub end_line: u32,
    /// Start byte offset.
    pub start_byte: u32,
    /// End byte offset.
    pub end_byte: u32,
    /// Kind of chunk.
    pub kind: ChunkKind,
    /// Identifier (symbol name or heading text).
    pub ident: String,
    /// Parent container (e.g. class name for a method, parent heading for subsection).
    pub parent: Option<String>,
    /// Function/method signature.
    pub signature: Option<String>,
    /// Visibility (pub, private, protected, etc.).
    pub visibility: Option<String>,
    /// UI context tag (pages, components, screens).
    pub ui_ctx: Option<String>,
    /// Doc comment preceding this item (///, /**, #, docstrings).
    #[serde(rename = "dc", skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
    /// Attributes/decorators/annotations (e.g. #[derive(Debug)], @Override).
    #[serde(rename = "at", skip_serializing_if = "Option::is_none")]
    pub attributes: Option<String>,
    /// Full content of the chunk.
    pub content: String,
}

impl Chunk {
    #[must_use]
    pub fn line_count(&self) -> u32 {
        self.end_line.saturating_sub(self.start_line) + 1
    }
}

/// A reference (call site, import, type usage) found in a chunk.
#[derive(Debug, Clone, Serialize)]
pub struct Reference {
    pub id: i64,
    pub chunk_id: i64,
    /// The identifier being referenced.
    pub target_ident: String,
    /// Kind of reference.
    pub ref_kind: RefKind,
    /// Line number where the reference occurs.
    pub line: u32,
    /// Column number.
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RefKind {
    Call,
    Import,
    TypeUse,
    FieldAccess,
}

impl RefKind {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Call => "call",
            Self::Import => "import",
            Self::TypeUse => "type_use",
            Self::FieldAccess => "field_access",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "call" => Self::Call,
            "import" => Self::Import,
            "type_use" => Self::TypeUse,
            "field_access" => Self::FieldAccess,
            _ => Self::Call,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_kind_round_trip() {
        let kinds = vec![
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
            let s = k.as_str();
            let back = ChunkKind::parse(s);
            assert_eq!(k, back);
        }
    }

    #[test]
    fn chunk_kind_other_round_trip() {
        let k = ChunkKind::Other("custom".into());
        assert_eq!(k.as_str(), "custom");
        let back = ChunkKind::parse("custom");
        assert_eq!(k, back);
    }

    #[test]
    fn chunk_line_count() {
        let c = Chunk {
            id: 0,
            file_id: 1,
            start_line: 5,
            end_line: 15,
            start_byte: 0,
            end_byte: 100,
            kind: ChunkKind::Function,
            ident: "foo".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: String::new(),
        };
        assert_eq!(c.line_count(), 11);
    }

    #[test]
    fn ref_kind_round_trip() {
        for k in [
            RefKind::Call,
            RefKind::Import,
            RefKind::TypeUse,
            RefKind::FieldAccess,
        ] {
            let s = k.as_str();
            let back = RefKind::parse(s);
            assert_eq!(k, back);
        }
    }
}
