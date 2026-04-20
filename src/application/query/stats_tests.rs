//! Tests for `stats.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "stats_tests.rs"] mod tests;`.

use super::{get_quality_info, get_stats, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

const TEST_FILE_SIZE: u64 = 1024;
const TEST_FILE_BYTES_SMALL: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn get_stats_basic() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_SIZE,
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
        ident: "test".into(),
        parent: None,
        signature: None,
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn test() {}".into(),
    };
    db.insert_chunk(&chunk).unwrap();

    let result = get_stats(&db).unwrap();
    assert_eq!(result.files, 1);
    assert_eq!(result.chunks, 1);
    assert_eq!(result.total_bytes, TEST_FILE_SIZE);
}

#[test]
fn get_quality_info_empty() {
    let db = test_db();
    let result = get_quality_info(&db).unwrap();
    assert!(result.is_none());
}

#[test]
fn get_quality_info_with_issues() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES_SMALL,
    );
    let file_id = db.upsert_file(&file).unwrap();
    db.set_file_parse_quality(file_id, "partial").unwrap();

    let result = get_quality_info(&db).unwrap();
    assert!(result.is_some());
    let info = result.unwrap();
    assert_eq!(info.files_with_parse_warnings, 1);
    assert_eq!(info.files[0].path, "src/lib.rs");
    assert_eq!(info.files[0].quality, "partial");
}
