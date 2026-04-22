//! Tests for `application::query::read`.
//!
//! Integration-tested end-to-end through `cli_tests` and `mcp_tests`
//! (every CLI + MCP read test exercises this module). Unit tests are
//! added here as specific edge cases surface (parent disambiguation,
//! section-not-found hints, …).

use super::{read_symbol, ReadSymbolInput};
use crate::db::Database;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;
use crate::error::RlmError;

fn make_db_with_two_news() -> Database {
    let db = Database::open_in_memory().unwrap();
    let file = FileRecord::new("src/lib.rs".into(), "h".into(), "rust".into(), 200);
    let file_id = db.upsert_file(&file).unwrap();

    // Two methods with the same ident `new`, different parents.
    for (parent, body) in [
        ("Foo", "fn new() -> Foo { Foo }"),
        ("Bar", "fn new() -> Bar { Bar }"),
    ] {
        db.insert_chunk(&Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 1,
            start_byte: 0,
            end_byte: body.len() as u32,
            kind: ChunkKind::Method,
            ident: "new".into(),
            parent: Some(parent.into()),
            signature: Some("fn new()".into()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: body.into(),
        })
        .unwrap();
    }
    db
}

/// When `--parent` is set and the requested file is wrong, the fallback
/// must still honour the parent filter — otherwise the disambiguation
/// flag is silently defeated and the agent gets every `new` in the
/// project mixed together.
#[test]
fn read_symbol_with_wrong_path_and_parent_filters_fallback_by_parent() {
    let db = make_db_with_two_news();
    let out = read_symbol(
        &db,
        &ReadSymbolInput {
            path: "src/does_not_exist.rs",
            symbol: "new",
            parent: Some("Foo"),
            metadata: false,
        },
    )
    .unwrap();

    assert!(
        out.body.contains("\"parent\":\"Foo\""),
        "fallback must include Foo::new: {}",
        out.body
    );
    assert!(
        !out.body.contains("\"parent\":\"Bar\""),
        "fallback must NOT include Bar::new when --parent=Foo: {}",
        out.body
    );
}

/// When `--parent` names a parent that doesn't exist anywhere in the
/// index, the fallback must error explicitly rather than dump every
/// match for the bare ident — silent fallback to "all" would hide the
/// typo.
#[test]
fn read_symbol_with_parent_not_found_anywhere_errors() {
    let db = make_db_with_two_news();
    let err = read_symbol(
        &db,
        &ReadSymbolInput {
            path: "src/lib.rs",
            symbol: "new",
            parent: Some("Nonexistent"),
            metadata: false,
        },
    )
    .unwrap_err();

    match err {
        RlmError::SymbolNotFound { ident } => assert_eq!(ident, "Nonexistent::new"),
        other => panic!("expected SymbolNotFound(\"Nonexistent::new\"), got {other:?}"),
    }
}

/// Existing behaviour preserved: without `--parent`, a wrong path still
/// falls back to every match for the ident — the "maybe you typed the
/// path wrong" affordance.
#[test]
fn read_symbol_wrong_path_without_parent_returns_all_matches() {
    let db = make_db_with_two_news();
    let out = read_symbol(
        &db,
        &ReadSymbolInput {
            path: "src/does_not_exist.rs",
            symbol: "new",
            parent: None,
            metadata: false,
        },
    )
    .unwrap();

    assert!(
        out.body.contains("\"parent\":\"Foo\"") && out.body.contains("\"parent\":\"Bar\""),
        "both matches should be returned when no parent is given: {}",
        out.body
    );
}
