//! Tests for `extractor.rs` (task #122).

use super::{extract_symbols, ExtractOutcome};
use crate::db::Database;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

fn setup_source(
    content: &str,
    chunks: Vec<(String, u32, u32, String)>,
) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("src.rs");
    std::fs::write(&path, content).unwrap();
    let db = Database::open_in_memory().unwrap();
    let rel = "src.rs".to_string();
    let f = FileRecord::new(rel, "h".into(), "rust".into(), content.len() as u64);
    let fid = db.upsert_file(&f).unwrap();
    for (ident, start, end, body) in chunks {
        let chunk = Chunk {
            kind: ChunkKind::Function,
            ident,
            start_line: 1,
            end_line: 3,
            start_byte: start,
            end_byte: end,
            content: body,
            ..Chunk::stub(fid)
        };
        db.insert_chunk(&chunk).unwrap();
    }
    (dir, db)
}

#[test]
fn extract_moves_single_symbol_to_new_file() {
    let body = "fn hello() -> &'static str { \"hi\" }";
    let source = format!("{body}\nfn other() {{}}\n");
    let start = 0_u32;
    let end = body.len() as u32;
    let (dir, db) = setup_source(&source, vec![("hello".into(), start, end, body.into())]);

    let outcome: ExtractOutcome = extract_symbols(
        &db,
        "src.rs",
        &["hello".to_string()],
        "extracted.rs",
        None,
        dir.path(),
    )
    .expect("extract should succeed");

    assert!(outcome.dest_created, "dest did not pre-exist → created");
    assert_eq!(outcome.moved.len(), 1);
    assert_eq!(outcome.moved[0].symbol, "hello");

    let dest = std::fs::read_to_string(dir.path().join("extracted.rs")).unwrap();
    assert!(
        dest.contains("fn hello()"),
        "dest should contain moved body, got: {dest:?}"
    );

    let src = std::fs::read_to_string(dir.path().join("src.rs")).unwrap();
    assert!(
        !src.contains("fn hello()"),
        "source should no longer contain hello, got: {src:?}"
    );
    assert!(
        src.contains("fn other()"),
        "other symbol should remain, got: {src:?}"
    );
}

#[test]
fn extract_moves_multiple_symbols_in_one_call() {
    let body_a = "fn alpha() {}";
    let body_b = "fn beta() {}";
    let source = format!("{body_a}\n{body_b}\nfn gamma() {{}}\n");
    let start_a = 0_u32;
    let end_a = body_a.len() as u32;
    let start_b = (body_a.len() + 1) as u32;
    let end_b = start_b + body_b.len() as u32;
    let (dir, db) = setup_source(
        &source,
        vec![
            ("alpha".into(), start_a, end_a, body_a.into()),
            ("beta".into(), start_b, end_b, body_b.into()),
        ],
    );

    let outcome = extract_symbols(
        &db,
        "src.rs",
        &["alpha".to_string(), "beta".to_string()],
        "moved.rs",
        None,
        dir.path(),
    )
    .unwrap();

    assert_eq!(outcome.moved.len(), 2);

    let dest = std::fs::read_to_string(dir.path().join("moved.rs")).unwrap();
    assert!(dest.contains("fn alpha()") && dest.contains("fn beta()"));

    let src = std::fs::read_to_string(dir.path().join("src.rs")).unwrap();
    assert!(!src.contains("fn alpha()") && !src.contains("fn beta()"));
    assert!(src.contains("fn gamma()"));
}

#[test]
fn extract_includes_doc_comment_by_default() {
    let body = "pub fn stub() {}";
    let source = format!("/// Important doc.\n{body}\n");
    let start = source.find(body).unwrap() as u32;
    let end = start + body.len() as u32;
    let (dir, db) = setup_source(&source, vec![("stub".into(), start, end, body.into())]);

    extract_symbols(
        &db,
        "src.rs",
        &["stub".to_string()],
        "docs_moved.rs",
        None,
        dir.path(),
    )
    .unwrap();

    let dest = std::fs::read_to_string(dir.path().join("docs_moved.rs")).unwrap();
    assert!(
        dest.contains("Important doc"),
        "doc comment should move with symbol, got: {dest:?}"
    );
    let src = std::fs::read_to_string(dir.path().join("src.rs")).unwrap();
    assert!(
        !src.contains("Important doc"),
        "doc should leave the source too, got: {src:?}"
    );
}

#[test]
fn extract_appends_to_existing_dest() {
    let body = "fn newcomer() {}";
    let source = format!("{body}\n");
    let (dir, db) = setup_source(
        &source,
        vec![("newcomer".into(), 0, body.len() as u32, body.into())],
    );
    // Pre-populate dest with content we must preserve.
    std::fs::write(dir.path().join("shared.rs"), "fn already_there() {}\n").unwrap();

    let outcome = extract_symbols(
        &db,
        "src.rs",
        &["newcomer".to_string()],
        "shared.rs",
        None,
        dir.path(),
    )
    .unwrap();

    assert!(
        !outcome.dest_created,
        "dest pre-existed → append, not create"
    );

    let dest = std::fs::read_to_string(dir.path().join("shared.rs")).unwrap();
    assert!(
        dest.contains("fn already_there()"),
        "existing content preserved"
    );
    assert!(dest.contains("fn newcomer()"), "new content appended");
}

#[test]
fn extract_rejects_unknown_symbol() {
    let body = "fn known() {}";
    let source = format!("{body}\n");
    let (dir, db) = setup_source(
        &source,
        vec![("known".into(), 0, body.len() as u32, body.into())],
    );

    let result = extract_symbols(
        &db,
        "src.rs",
        &["ghost".to_string()],
        "never_created.rs",
        None,
        dir.path(),
    );
    assert!(result.is_err(), "unknown symbol must error");
    assert!(
        !dir.path().join("never_created.rs").exists(),
        "on error, dest must not have been written"
    );
}

#[test]
fn extract_rejects_empty_symbols_list() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in_memory().unwrap();
    let result = extract_symbols(&db, "src.rs", &[], "dest.rs", None, dir.path());
    assert!(result.is_err(), "empty symbol list must error");
}
