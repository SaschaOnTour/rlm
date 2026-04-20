//! DTOs for chunk / reference types.

use serde::Serialize;

use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};

/// Wire-format of `ChunkKind`. Matches the JSON shape produced before
/// the domain / DTO split so existing MCP clients see no change.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ChunkKindDto {
    Named(&'static str),
    Other(String),
}

impl From<&ChunkKind> for ChunkKindDto {
    fn from(kind: &ChunkKind) -> Self {
        // The domain `ChunkKind::as_str` returns `&str` (needed for
        // shorter-lifetime views); the wire DTO wants `&'static str`
        // so the JSON payload doesn't have to clone the tag. Inline
        // the match here — the domain side's `as_str` and this arm
        // list stay in sync by contract of the `ChunkKind` enum.
        match kind {
            ChunkKind::Function => Self::Named("fn"),
            ChunkKind::Method => Self::Named("method"),
            ChunkKind::Struct => Self::Named("struct"),
            ChunkKind::Enum => Self::Named("enum"),
            ChunkKind::Trait => Self::Named("trait"),
            ChunkKind::Impl => Self::Named("impl"),
            ChunkKind::Class => Self::Named("class"),
            ChunkKind::Interface => Self::Named("interface"),
            ChunkKind::Module => Self::Named("mod"),
            ChunkKind::Constant => Self::Named("const"),
            ChunkKind::Section => Self::Named("section"),
            ChunkKind::Page => Self::Named("page"),
            ChunkKind::Other(s) => Self::Other(s.clone()),
        }
    }
}

/// Wire-format of `Chunk`.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkDto {
    pub id: i64,
    pub file_id: i64,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub kind: ChunkKindDto,
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

/// Consume a `Chunk` into a `ChunkDto`. Owning conversion avoids the
/// field-for-field clones that BP-008 rightly flags as boilerplate.
/// Callers that have `&Chunk` must `.clone()` at the call site,
/// which makes the copy explicit rather than hidden inside the
/// conversion.
impl From<Chunk> for ChunkDto {
    fn from(c: Chunk) -> Self {
        Self {
            id: c.id,
            file_id: c.file_id,
            start_line: c.start_line,
            end_line: c.end_line,
            start_byte: c.start_byte,
            end_byte: c.end_byte,
            kind: (&c.kind).into(),
            ident: c.ident,
            parent: c.parent,
            signature: c.signature,
            visibility: c.visibility,
            ui_ctx: c.ui_ctx,
            doc_comment: c.doc_comment,
            attributes: c.attributes,
            content: c.content,
        }
    }
}

/// Wire-format of `RefKind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RefKindDto {
    Call,
    Import,
    TypeUse,
    FieldAccess,
}

impl From<&RefKind> for RefKindDto {
    fn from(kind: &RefKind) -> Self {
        match kind {
            RefKind::Call => Self::Call,
            RefKind::Import => Self::Import,
            RefKind::TypeUse => Self::TypeUse,
            RefKind::FieldAccess => Self::FieldAccess,
        }
    }
}

/// Wire-format of `Reference`.
#[derive(Debug, Clone, Serialize)]
pub struct ReferenceDto {
    pub id: i64,
    pub chunk_id: i64,
    pub target_ident: String,
    pub ref_kind: RefKindDto,
    pub line: u32,
    pub col: u32,
}

impl From<Reference> for ReferenceDto {
    fn from(r: Reference) -> Self {
        Self {
            id: r.id,
            chunk_id: r.chunk_id,
            target_ident: r.target_ident,
            ref_kind: (&r.ref_kind).into(),
            line: r.line,
            col: r.col,
        }
    }
}

#[cfg(test)]
#[path = "chunk_dto_tests.rs"]
mod tests;
