//! Tests for `scope.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "scope_tests.rs"] mod tests;`.

use super::{get_scope, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const BAR_START_LINE: u32 = 7;
const BAR_END_LINE: u32 = 15;
const BAR_START_BYTE: u32 = 51;
const BAR_END_BYTE: u32 = 150;
const QUERY_LINE: u32 = 10;

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn get_scope_basic() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    // First function
    let chunk1 = Chunk {
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "foo".into(),
        content: "fn foo() {}".into(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&chunk1).unwrap();

    // Second function
    let chunk2 = Chunk {
        start_line: BAR_START_LINE,
        end_line: BAR_END_LINE,
        start_byte: BAR_START_BYTE,
        end_byte: BAR_END_BYTE,
        kind: ChunkKind::Function,
        ident: "bar".into(),
        content: "fn bar() {}".into(),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&chunk2).unwrap();

    // Query at line QUERY_LINE (inside bar)
    let result = get_scope(&db, "src/lib.rs", QUERY_LINE).unwrap();
    assert_eq!(result.file, "src/lib.rs");
    assert_eq!(result.line, QUERY_LINE);
    assert_eq!(result.containing, vec!["bar"]);
    // Both foo and bar are visible (defined before line 10)
    assert!(result.visible.contains(&"fn:foo".to_string()));
    assert!(result.visible.contains(&"fn:bar".to_string()));
}

#[test]
fn get_scope_file_not_found() {
    let db = test_db();
    let result = get_scope(&db, "nonexistent.rs", 1);
    assert!(result.is_err());
}
