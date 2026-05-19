//! Tests for `application::query::read`.
//!
//! Integration-tested end-to-end through `cli_tests` and `mcp_tests`
//! (every CLI + MCP read test exercises this module). Unit tests are
//! added here as specific edge cases surface (parent disambiguation,
//! section-not-found hints, …).

use super::{read_symbol, ReadInputs, ReadRequest, ReadSymbolInput};
use crate::db::Database;
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;
use crate::error::RlmError;
use std::path::Path;

/// Most tests in this module exercise the non-metadata path, where
/// `project_root` is only used to thread through — the parent-aware
/// ref count never runs. An empty path keeps the test setup minimal
/// without affecting outcomes; the dedicated `ref_count_*` test below
/// sets up a real tempdir for the cases that do read source files.
const TEST_PROJECT_ROOT: &str = "";

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
        Path::new(TEST_PROJECT_ROOT),
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
        Path::new(TEST_PROJECT_ROOT),
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

/// Two files both define `Foo::new`. Reading from `tests/fixture.rs`
/// with `--metadata` must report metadata about the chunk it actually
/// returns — not bleed `src/lib.rs`'s `Foo::new` signature/file into
/// the response. Before the chunk-scoped derivation, `type_info` was
/// computed by a global `(symbol, parent)` query and got its `file`
/// from src/lib.rs (priority pass), while `signature.signatures`
/// listed entries from both files. This test pins the scoped shape.
#[test]
fn metadata_scopes_to_the_returned_chunks_when_parent_repeats_across_files() {
    let db = Database::open_in_memory().unwrap();

    // Shared baseline for both `Foo::new` chunks; per-file values are
    // overridden via the struct-update syntax below. Cuts the
    // duplication rustqual's BP-009 flags when the same struct gets
    // constructed twice with overlapping fields.
    let foo_new_base = Chunk {
        id: 0,
        file_id: 0,
        start_line: 0,
        end_line: 0,
        start_byte: 0,
        end_byte: 0,
        kind: ChunkKind::Method,
        ident: "new".into(),
        parent: Some("Foo".into()),
        signature: None,
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: String::new(),
    };

    let lib_file = FileRecord::new("src/lib.rs".into(), "h1".into(), "rust".into(), 200);
    let lib_id = db.upsert_file(&lib_file).unwrap();
    db.insert_chunk(&Chunk {
        file_id: lib_id,
        start_line: 10,
        end_line: 12,
        start_byte: 100,
        end_byte: 140,
        signature: Some("fn new() -> Lib".into()),
        content: "fn new() -> Lib { Lib }".into(),
        ..foo_new_base.clone()
    })
    .unwrap();

    let fix_file = FileRecord::new("tests/fixture.rs".into(), "h2".into(), "rust".into(), 100);
    let fix_id = db.upsert_file(&fix_file).unwrap();
    db.insert_chunk(&Chunk {
        file_id: fix_id,
        start_line: 5,
        end_line: 7,
        start_byte: 50,
        end_byte: 95,
        signature: Some("fn new() -> Fixture".into()),
        content: "fn new() -> Fixture { Fixture }".into(),
        ..foo_new_base
    })
    .unwrap();

    let out = read_symbol(
        &db,
        Path::new(TEST_PROJECT_ROOT),
        &ReadSymbolInput {
            path: "tests/fixture.rs",
            symbol: "new",
            parent: Some("Foo"),
            metadata: true,
        },
    )
    .unwrap();

    // type_info must point at the file the user actually read.
    assert!(
        out.body.contains("\"file\":\"tests/fixture.rs\""),
        "type_info.file must be the read file, got body: {}",
        out.body
    );
    assert!(
        !out.body.contains("\"file\":\"src/lib.rs\""),
        "type_info must not leak src/lib.rs into a tests/fixture.rs read: {}",
        out.body
    );

    // signatures must come only from the chunk(s) the read returned.
    assert!(
        out.body.contains("fn new() -> Fixture"),
        "signature for the read chunk must be present: {}",
        out.body
    );
    assert!(
        !out.body.contains("fn new() -> Lib"),
        "signature from the other file's Foo::new must not leak: {}",
        out.body
    );
}

/// `ref_count` must reflect the parent the user asked about. Without
/// scoping, the legacy code counts every ref to ident `new` across
/// the project — including `Bar::new` callers — so `--parent Foo`
/// inflates by an order of magnitude. The fix uses parent-aware
/// impact analysis (column-aware path-call resolution) to count only
/// refs that pertain to `Foo::new`.
///
/// `filter_impacted_by_parent` resolves path calls by reading the
/// caller's source at the recorded line/col, so the caller file has
/// to physically exist under `project_root`. We use a tempdir for
/// that and pass its path through.
#[test]
fn ref_count_with_parent_excludes_other_parents_calls() {
    let db = make_db_with_two_news();

    let tmp = tempfile::tempdir().unwrap();
    let caller_rel = "src/caller.rs";
    let caller_src = "fn use_news() {\n    Foo::new();\n    Bar::new();\n    new();\n}\n";
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join(caller_rel), caller_src).unwrap();

    let caller_file = FileRecord::new(caller_rel.into(), "h3".into(), "rust".into(), 50);
    let caller_id = db.upsert_file(&caller_file).unwrap();
    let caller_chunk_id = db
        .insert_chunk(&Chunk {
            id: 0,
            file_id: caller_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: caller_src.len() as u32,
            kind: ChunkKind::Function,
            ident: "use_news".into(),
            parent: None,
            signature: Some("fn use_news()".into()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: caller_src.into(),
        })
        .unwrap();

    // Columns are 0-indexed (tree-sitter convention) and point at the
    // ident itself — for `    Foo::new()` that's column 9, the start
    // of `new`. Same for `Bar::new()`. For bare `    new()` it's 4.
    for (line, col) in [(2, 9), (3, 9), (4, 4)] {
        db.insert_ref(&Reference {
            id: 0,
            chunk_id: caller_chunk_id,
            target_ident: "new".into(),
            ref_kind: RefKind::Call,
            line,
            col,
        })
        .unwrap();
    }

    let out = read_symbol(
        &db,
        tmp.path(),
        &ReadSymbolInput {
            path: "src/lib.rs",
            symbol: "new",
            parent: Some("Foo"),
            metadata: true,
        },
    )
    .unwrap();

    // The unscoped count would be 3. The parent-aware count for Foo
    // is 1 (only `Foo::new()` survives the path-call filter — bare
    // `new()` and `Bar::new()` are dropped).
    assert!(
        out.body.contains("\"ref_count\":1"),
        "ref_count must be parent-scoped to 1 (only Foo::new()), got body: {}",
        out.body
    );
    assert!(
        !out.body.contains("\"ref_count\":3"),
        "ref_count must not include Bar::new() or bare new() calls: {}",
        out.body
    );
}

/// Contract pin: `ref_count` is **parent-wide**, not
/// **selected-definition-scoped**. When two files both define
/// `Foo::new`, rlm has no way to tell at the ref level which
/// definition a `Foo::new()` call resolves to — refs carry only
/// `target_ident`, not a definition fingerprint. So a single
/// project-wide `Foo::new()` call site contributes `ref_count: 1`
/// regardless of which file the user reads from. The signature view
/// describes the *parent::symbol pair* across the project, not the
/// callers of the specific definition the read returned.
///
/// We document this rather than fix it: the structurally correct fix
/// (attributing each call to a concrete target) needs full type-flow
/// analysis, which rlm doesn't do. The honest contract — "calls to
/// `Foo::new` exist project-wide; attribution to a specific
/// definition is ambiguous when multiple definitions exist" — is
/// what the `SignatureResult::ref_count` doc string asserts and what
/// this test pins.
#[test]
fn ref_count_is_parent_wide_not_definition_scoped() {
    let db = Database::open_in_memory().unwrap();

    let foo_new_base = Chunk {
        id: 0,
        file_id: 0,
        start_line: 0,
        end_line: 0,
        start_byte: 0,
        end_byte: 0,
        kind: ChunkKind::Method,
        ident: "new".into(),
        parent: Some("Foo".into()),
        signature: Some("fn new()".into()),
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: String::new(),
    };

    let lib_file = FileRecord::new("src/lib.rs".into(), "h1".into(), "rust".into(), 200);
    let lib_id = db.upsert_file(&lib_file).unwrap();
    db.insert_chunk(&Chunk {
        file_id: lib_id,
        ..foo_new_base.clone()
    })
    .unwrap();

    let fix_file = FileRecord::new("tests/fixture.rs".into(), "h2".into(), "rust".into(), 100);
    let fix_id = db.upsert_file(&fix_file).unwrap();
    db.insert_chunk(&Chunk {
        file_id: fix_id,
        ..foo_new_base.clone()
    })
    .unwrap();

    // Single caller — calls `Foo::new()` once in src/lib.rs (cannot
    // be unambiguously attributed to either definition).
    let tmp = tempfile::tempdir().unwrap();
    let caller_rel = "src/caller.rs";
    let caller_src = "fn use_news() {\n    Foo::new();\n}\n";
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join(caller_rel), caller_src).unwrap();
    let caller_file = FileRecord::new(caller_rel.into(), "h3".into(), "rust".into(), 30);
    let caller_id = db.upsert_file(&caller_file).unwrap();
    let caller_chunk_id = db
        .insert_chunk(&Chunk {
            file_id: caller_id,
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: caller_src.len() as u32,
            kind: ChunkKind::Function,
            ident: "use_news".into(),
            parent: None,
            content: caller_src.into(),
            ..foo_new_base.clone() // shape only — overrides above set the real values
        })
        .unwrap();
    db.insert_ref(&Reference {
        id: 0,
        chunk_id: caller_chunk_id,
        target_ident: "new".into(),
        ref_kind: RefKind::Call,
        line: 2,
        col: 9,
    })
    .unwrap();

    // Reading from EITHER file with `--parent Foo` must report the
    // same parent-wide count: 1. The reviewer's expectation that
    // reading from `tests/fixture.rs` should yield `ref_count: 0`
    // (since the only caller is in src/) cannot be satisfied at the
    // ref-resolution layer — and we document that explicitly.
    for read_path in ["src/lib.rs", "tests/fixture.rs"] {
        let out = read_symbol(
            &db,
            tmp.path(),
            &ReadSymbolInput {
                path: read_path,
                symbol: "new",
                parent: Some("Foo"),
                metadata: true,
            },
        )
        .unwrap();

        assert!(
            out.body.contains("\"ref_count\":1"),
            "ref_count must be parent-wide (1) regardless of which \
             Foo::new definition the read targeted (tried {read_path}). \
             body: {}",
            out.body,
        );
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
        Path::new(TEST_PROJECT_ROOT),
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

#[test]
fn from_optional_inputs_with_only_symbol_builds_symbol_variant() {
    let inputs = ReadInputs {
        path: "p.rs",
        symbol: Some("sym"),
        section: None,
        parent: None,
        metadata: false,
    };
    let req = ReadRequest::from_optional_inputs(&inputs).unwrap();
    assert!(matches!(req, ReadRequest::Symbol(_)));
}

#[test]
fn from_optional_inputs_with_only_section_builds_section_variant() {
    let inputs = ReadInputs {
        path: "p.md",
        symbol: None,
        section: Some("Heading"),
        parent: None,
        metadata: false,
    };
    let req = ReadRequest::from_optional_inputs(&inputs).unwrap();
    assert!(matches!(req, ReadRequest::Section { .. }));
}

#[test]
fn from_optional_inputs_with_both_errors() {
    let inputs = ReadInputs {
        path: "p.rs",
        symbol: Some("sym"),
        section: Some("Head"),
        parent: None,
        metadata: false,
    };
    let err = ReadRequest::from_optional_inputs(&inputs)
        .err()
        .expect("both symbol+section should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("exactly one of 'symbol' or 'section'"),
        "got: {msg}"
    );
}

#[test]
fn from_optional_inputs_with_neither_errors() {
    let inputs = ReadInputs {
        path: "p.rs",
        symbol: None,
        section: None,
        parent: None,
        metadata: false,
    };
    let err = ReadRequest::from_optional_inputs(&inputs)
        .err()
        .expect("neither symbol nor section should be rejected");
    let msg = err.to_string();
    assert!(msg.contains("requires 'symbol' or 'section'"), "got: {msg}");
}
