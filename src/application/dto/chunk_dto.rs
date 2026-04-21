//! DTOs for chunk / reference types.
//!
//! The adapter boundary (CLI JSON output, MCP tool responses) needs
//! `Serialize` but the domain types stay format-free. Rather than clone
//! every `Chunk`/`Reference` to hand it to serde, the DTOs here **borrow**
//! from the domain value via a shared lifetime — `From<&Chunk>` produces
//! a `ChunkDto<'_>` that aliases the source strings, so JSON emission is
//! zero-copy on the payload fields (`ident`, `content`, `signature`, …).
//!
//! Only small `Copy` fields (line/byte numbers) and the `ChunkKindDto`
//! tag are materialised during the conversion.

use serde::Serialize;

use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};

/// Wire-format of `ChunkKind`. Matches the JSON shape produced before
/// the domain / DTO split so existing MCP clients see no change.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ChunkKindDto<'a> {
    Named(&'static str),
    Other(&'a str),
}

impl<'a> From<&'a ChunkKind> for ChunkKindDto<'a> {
    fn from(kind: &'a ChunkKind) -> Self {
        // The domain `ChunkKind::as_str` returns `&str` (needed for
        // shorter-lifetime views); the wire DTO wants `&'static str`
        // on the `Named` path so the JSON payload doesn't have to clone
        // the tag. Inline the match here — the domain side's `as_str`
        // and this arm list stay in sync by contract of the
        // `ChunkKind` enum.
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
            ChunkKind::Other(s) => Self::Other(s.as_str()),
        }
    }
}

/// Wire-format of `Chunk`. Borrows all string fields from the source
/// `Chunk` so no payload copy happens on the serialization path.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkDto<'a> {
    pub id: i64,
    pub file_id: i64,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub kind: ChunkKindDto<'a>,
    pub ident: &'a str,
    pub parent: Option<&'a str>,
    pub signature: Option<&'a str>,
    pub visibility: Option<&'a str>,
    pub ui_ctx: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<&'a str>,
    pub content: &'a str,
}

impl<'a> From<&'a Chunk> for ChunkDto<'a> {
    fn from(c: &'a Chunk) -> Self {
        Self {
            id: c.id,
            file_id: c.file_id,
            start_line: c.start_line,
            end_line: c.end_line,
            start_byte: c.start_byte,
            end_byte: c.end_byte,
            kind: (&c.kind).into(),
            ident: &c.ident,
            parent: c.parent.as_deref(),
            signature: c.signature.as_deref(),
            visibility: c.visibility.as_deref(),
            ui_ctx: c.ui_ctx.as_deref(),
            doc_comment: c.doc_comment.as_deref(),
            attributes: c.attributes.as_deref(),
            content: &c.content,
        }
    }
}

/// Wire-format of `RefKind`. All variants are unit enum → no borrowed data.
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

/// Wire-format of `Reference`. Same zero-copy story as `ChunkDto`.
#[derive(Debug, Clone, Serialize)]
pub struct ReferenceDto<'a> {
    pub id: i64,
    pub chunk_id: i64,
    pub target_ident: &'a str,
    pub ref_kind: RefKindDto,
    pub line: u32,
    pub col: u32,
}

impl<'a> From<&'a Reference> for ReferenceDto<'a> {
    fn from(r: &'a Reference) -> Self {
        Self {
            id: r.id,
            chunk_id: r.chunk_id,
            target_ident: &r.target_ident,
            ref_kind: (&r.ref_kind).into(),
            line: r.line,
            col: r.col,
        }
    }
}

#[cfg(test)]
#[path = "chunk_dto_tests.rs"]
mod tests;
