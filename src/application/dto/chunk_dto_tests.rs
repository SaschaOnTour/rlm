//! Tests for the Chunk / Reference DTOs.
//!
//! The domain-side roundtrip tests retired in slice 6.5 moved here:
//! they now verify the adapter wire contract instead of "can the
//! domain type round-trip through serde", which is the right target
//! for a serialisation test.

use super::{ChunkDto, ChunkKindDto, RefKindDto, ReferenceDto};
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};

#[test]
fn chunk_kind_named_variant_serialises_as_bare_string() {
    let dto: ChunkKindDto = (&ChunkKind::Function).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert_eq!(json, "\"fn\"");
}

#[test]
fn chunk_kind_other_variant_serialises_as_its_inner_string() {
    let dto: ChunkKindDto = (&ChunkKind::Other("macro".into())).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert_eq!(json, "\"macro\"");
}

#[test]
fn chunk_dto_round_trips_core_fields() {
    let chunk = Chunk {
        id: 42,
        file_id: 7,
        start_line: 10,
        end_line: 15,
        start_byte: 100,
        end_byte: 200,
        kind: ChunkKind::Function,
        ident: "hello".into(),
        parent: None,
        signature: Some("fn hello()".into()),
        visibility: Some("pub".into()),
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn hello() {}".into(),
    };
    let dto = ChunkDto::from(chunk);
    assert_eq!(dto.id, 42);
    assert_eq!(dto.ident, "hello");
    assert_eq!(dto.signature.as_deref(), Some("fn hello()"));

    let json = serde_json::to_string(&dto).unwrap();
    // Fields that were present survive the wire trip.
    assert!(json.contains("\"ident\":\"hello\""));
    assert!(json.contains("\"signature\":\"fn hello()\""));
    // `doc_comment` / `attributes` were None; the `skip_serializing_if`
    // attribute keeps them out of the JSON.
    assert!(!json.contains("doc_comment"));
    assert!(!json.contains("attributes"));
}

#[test]
fn reference_dto_mirrors_domain_reference() {
    let r = Reference {
        id: 3,
        chunk_id: 11,
        target_ident: "do_thing".into(),
        ref_kind: RefKind::Call,
        line: 42,
        col: 8,
    };
    let dto = ReferenceDto::from(r);
    assert_eq!(dto.id, 3);
    assert_eq!(dto.chunk_id, 11);
    assert_eq!(dto.target_ident, "do_thing");
    assert_eq!(dto.ref_kind, RefKindDto::Call);
    assert_eq!(dto.line, 42);
    assert_eq!(dto.col, 8);
}
