//! Tests for `peek.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "peek_tests.rs"] mod tests;`.

use super::{peek, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

/// File size in bytes for test file records.
const TEST_FILE_SIZE: u64 = 100;
/// End line of the test chunk.
const CHUNK_END_LINE: u32 = 5;
/// End byte offset of the test chunk.
const CHUNK_END_BYTE: u32 = 50;

#[test]
fn peek_returns_structure_no_content() {
    let db = Database::open_in_memory().unwrap();
    let f = FileRecord::new(
        "src/main.rs".into(),
        "h".into(),
        "rust".into(),
        TEST_FILE_SIZE,
    );
    let fid = db.upsert_file(&f).unwrap();
    let c = Chunk {
        id: 0,
        file_id: fid,
        start_line: 1,
        end_line: CHUNK_END_LINE,
        start_byte: 0,
        end_byte: CHUNK_END_BYTE,
        kind: ChunkKind::Function,
        ident: "main".into(),
        parent: None,
        signature: None,
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn main() { ... }".into(),
    };
    db.insert_chunk(&c).unwrap();

    let result = peek(&db, None).unwrap();
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].symbols.len(), 1);
    assert_eq!(result.files[0].symbols[0].name, "main");

    // Verify no content is in the output
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("fn main()"));
}

#[test]
fn peek_with_path_filter() {
    let db = Database::open_in_memory().unwrap();
    db.upsert_file(&FileRecord::new(
        "src/a.rs".into(),
        "h1".into(),
        "rust".into(),
        TEST_FILE_SIZE,
    ))
    .unwrap();
    db.upsert_file(&FileRecord::new(
        "lib/b.rs".into(),
        "h2".into(),
        "rust".into(),
        TEST_FILE_SIZE,
    ))
    .unwrap();

    let result = peek(&db, Some("src/")).unwrap();
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].path, "src/a.rs");
}
