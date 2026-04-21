//! Basic tests for `map.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "map_tests.rs"] mod tests;`.
//!
//! Advanced tests (multi-file, visibility filtering) live in the sibling
//! `map_advanced_tests.rs`.

use super::super::fixtures::setup_test_db;
use super::build_map;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 500;
const TEST_FILE_BYTES_SMALL: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 10;
const TEST_END_LINE_SHORT: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 100;
const TEST_END_BYTE_SMALL: u32 = 50;
const PRIV_FN_START_LINE: u32 = 15;
const PRIV_FN_END_LINE: u32 = 20;
const PRIV_FN_START_BYTE: u32 = 150;
const PRIV_FN_END_BYTE: u32 = 200;
const STRUCT_START_LINE: u32 = 25;
const STRUCT_END_LINE: u32 = 30;
const STRUCT_START_BYTE: u32 = 250;
const STRUCT_END_BYTE: u32 = 300;

#[test]
fn test_map_empty_db() {
    let db = setup_test_db();
    let result = build_map(&db, None).unwrap();

    assert!(result.results.is_empty());
}

#[test]
fn test_map_basic() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/lib.rs".to_string(),
        "abc123".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let pub_fn = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "process".to_string(),
        signature: Some("fn process()".to_string()),
        visibility: Some("pub".to_string()),
        content: "pub fn process() { }".to_string(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&pub_fn).unwrap();

    let priv_fn = Chunk {
        start_line: PRIV_FN_START_LINE,
        end_line: PRIV_FN_END_LINE,
        start_byte: PRIV_FN_START_BYTE,
        end_byte: PRIV_FN_END_BYTE,
        kind: ChunkKind::Function,
        ident: "helper".to_string(),
        signature: Some("fn helper()".to_string()),
        content: "fn helper() { }".to_string(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&priv_fn).unwrap();

    let pub_struct = Chunk {
        start_line: STRUCT_START_LINE,
        end_line: STRUCT_END_LINE,
        start_byte: STRUCT_START_BYTE,
        end_byte: STRUCT_END_BYTE,
        kind: ChunkKind::Struct,
        ident: "Config".to_string(),
        signature: Some("struct Config".to_string()),
        visibility: Some("pub".to_string()),
        content: "pub struct Config { }".to_string(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&pub_struct).unwrap();

    let result = build_map(&db, None).unwrap();

    assert_eq!(result.results.len(), 1);
    let entry = &result.results[0];
    assert_eq!(entry.file, "src/lib.rs");
    assert_eq!(entry.lang, "rust");
    assert_eq!(entry.line_count, STRUCT_END_LINE);
    assert_eq!(entry.symbols.len(), 2);
    assert!(entry.symbols.contains(&"fn:process".to_string()));
    assert!(entry.symbols.contains(&"struct:Config".to_string()));
    assert!(entry.description.contains("fn"));
    assert!(entry.description.contains("struct"));
}

#[test]
fn test_map_with_path_filter() {
    let db = setup_test_db();

    let file1 = FileRecord::new(
        "src/lib.rs".to_string(),
        "aaa".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES_SMALL,
    );
    let file1_id = db.upsert_file(&file1).unwrap();

    let file2 = FileRecord::new(
        "tests/test.rs".to_string(),
        "bbb".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES_SMALL,
    );
    let file2_id = db.upsert_file(&file2).unwrap();

    let chunk1 = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "main".to_string(),
        signature: Some("fn main()".to_string()),
        visibility: Some("pub".to_string()),
        content: "fn main() { }".to_string(),
        ..Chunk::stub(file1_id)
    };
    db.insert_chunk(&chunk1).unwrap();

    let chunk2 = Chunk {
        id: 0,
        file_id: file2_id,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Function,
        ident: "test_fn".to_string(),
        parent: None,
        signature: Some("fn test_fn()".to_string()),
        visibility: Some("pub".to_string()),
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn test_fn() { }".to_string(),
    };
    db.insert_chunk(&chunk2).unwrap();

    let result = build_map(&db, Some("src/")).unwrap();

    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].file, "src/lib.rs");
}
