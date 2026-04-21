//! Tests for `chunk.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "chunk_tests.rs"] mod tests;`.

use super::{Chunk, ChunkKind, RefKind};

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
