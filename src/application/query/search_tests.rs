//! Tests for `search.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "search_tests.rs"] mod tests;`.

use super::{run_fts, sanitize_fts_query, search_chunks, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TEST_SEARCH_LIMIT: usize = 10;

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn search_basic() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let chunk = Chunk {
        id: 0,
        file_id,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "search_test".into(),
        parent: None,
        signature: Some("fn search_test()".into()),
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn search_test() { println!(\"hello\"); }".into(),
    };
    db.insert_chunk(&chunk).unwrap();

    let result = search_chunks(&db, "search_test", TEST_SEARCH_LIMIT).unwrap();
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].name, "search_test");
    assert_eq!(result.results[0].kind, "fn");
    assert_eq!(result.file_count, 1);
}

#[test]
fn search_no_results() {
    let db = test_db();
    let result = search_chunks(&db, "nonexistent_xyz_123", TEST_SEARCH_LIMIT).unwrap();
    assert!(result.results.is_empty());
    assert_eq!(result.file_count, 0);
}

#[test]
fn sanitize_fts_query_basic() {
    let result = sanitize_fts_query("hello world");
    assert!(result.contains("\"hello\""));
    assert!(result.contains("\"world\""));
}

#[test]
fn sanitize_fts_query_special_chars() {
    let result = sanitize_fts_query("fn main() {}");
    assert!(result.contains("\"fn\""));
    assert!(result.contains("\"main\""));
}

#[test]
fn sanitize_fts_query_empty() {
    assert_eq!(sanitize_fts_query(""), "");
}

#[test]
fn run_fts_empty_db_returns_empty() {
    let db = test_db();
    let results = run_fts(&db, "hello", TEST_SEARCH_LIMIT).unwrap();
    assert!(results.is_empty());
}

#[test]
fn file_count_deduplicates_hits_in_same_file() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    // Two distinct chunks in the SAME file, both matching the query.
    for ident in ["foo_alpha", "foo_beta"] {
        let c = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE,
            kind: ChunkKind::Function,
            ident: ident.into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: format!("fn {ident}() {{}}"),
        };
        db.insert_chunk(&c).unwrap();
    }

    let result = search_chunks(&db, "foo", TEST_SEARCH_LIMIT).unwrap();
    assert_eq!(result.results.len(), 2);
    // Two hits in one file → one distinct file.
    assert_eq!(result.file_count, 1);
}
