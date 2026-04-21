//! Advanced map tests for `map.rs` (multi-file + visibility).
//!
//! Split out of `map_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE).

use super::super::fixtures::setup_test_db;
use super::build_map;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES_SMALL: u64 = 100;
const TEST_FILE_BYTES_MEDIUM: u64 = 200;
const TEST_END_LINE: u32 = 10;
const TEST_END_LINE_SHORT: u32 = 5;
const PRIV_FN_START_LINE: u32 = 15;
const PRIV_FN_END_LINE: u32 = 20;
const PUB_METHOD_START_BYTE: u32 = 50;
const PUB_METHOD_END_BYTE: u32 = 150;
const PRIV_METHOD_START_BYTE: u32 = 200;
const PRIV_METHOD_END_BYTE: u32 = 300;

#[test]
fn test_map_multiple_files() {
    let db = setup_test_db();

    let file1 = FileRecord::new(
        "src/a.rs".to_string(),
        "aaa".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES_SMALL,
    );
    db.upsert_file(&file1).unwrap();

    let file2 = FileRecord::new(
        "src/b.rs".to_string(),
        "bbb".to_string(),
        "rust".to_string(),
        TEST_FILE_BYTES_SMALL,
    );
    db.upsert_file(&file2).unwrap();

    let result = build_map(&db, None).unwrap();

    assert_eq!(result.results.len(), 2);
    let files: Vec<&str> = result.results.iter().map(|e| e.file.as_str()).collect();
    assert!(files.contains(&"src/a.rs"));
    assert!(files.contains(&"src/b.rs"));
}

#[test]
fn test_map_public_visibility() {
    let db = setup_test_db();

    let file = FileRecord::new(
        "src/lib.rs".to_string(),
        "xyz".to_string(),
        "java".to_string(),
        TEST_FILE_BYTES_MEDIUM,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let pub_method = Chunk {
        start_line: TEST_END_LINE_SHORT,
        end_line: TEST_END_LINE,
        start_byte: PUB_METHOD_START_BYTE,
        end_byte: PUB_METHOD_END_BYTE,
        kind: ChunkKind::Method,
        ident: "process".to_string(),
        parent: Some("MyClass".to_string()),
        signature: Some("public void process()".to_string()),
        visibility: Some("public".to_string()),
        content: "public void process() { }".to_string(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&pub_method).unwrap();

    let priv_method = Chunk {
        id: 0,
        file_id,
        start_line: PRIV_FN_START_LINE,
        end_line: PRIV_FN_END_LINE,
        start_byte: PRIV_METHOD_START_BYTE,
        end_byte: PRIV_METHOD_END_BYTE,
        kind: ChunkKind::Method,
        ident: "helper".to_string(),
        parent: Some("MyClass".to_string()),
        signature: Some("private void helper()".to_string()),
        visibility: Some("private".to_string()),
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "private void helper() { }".to_string(),
    };
    db.insert_chunk(&priv_method).unwrap();

    let result = build_map(&db, None).unwrap();

    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].symbols.len(), 1);
    assert!(result.results[0]
        .symbols
        .contains(&"method:process".to_string()));
}
