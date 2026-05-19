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
    assert_eq!(result.containing.len(), 1);
    assert_eq!(result.containing[0].ident, "bar");
    assert!(result.containing[0].parent.is_none());
    // Both foo and bar are visible (defined before line 10)
    let visible_idents: Vec<&str> = result.visible.iter().map(|s| s.ident.as_str()).collect();
    assert!(visible_idents.contains(&"foo"));
    assert!(visible_idents.contains(&"bar"));
    let foo_entry = result
        .visible
        .iter()
        .find(|s| s.ident == "foo")
        .expect("foo must be in visible");
    assert_eq!(foo_entry.kind, "fn");
}

#[test]
fn get_scope_carries_parent_for_methods() {
    let db = test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();

    let method = Chunk {
        start_line: 5,
        end_line: 15,
        kind: ChunkKind::Method,
        ident: "render".into(),
        parent: Some("Widget".into()),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&method).unwrap();

    let result = get_scope(&db, "src/x.rs", 10).unwrap();
    assert_eq!(result.containing.len(), 1);
    assert_eq!(result.containing[0].ident, "render");
    assert_eq!(result.containing[0].parent.as_deref(), Some("Widget"));
    let visible = &result.visible[0];
    assert_eq!(visible.ident, "render");
    assert_eq!(visible.parent.as_deref(), Some("Widget"));
    assert_eq!(visible.kind, "method");

    let json = serde_json::to_string(&result).unwrap();
    assert!(
        json.contains("\"parent\":\"Widget\""),
        "method scope entries must serialise their parent, got {json}",
    );
}

#[test]
fn get_scope_file_not_found() {
    let db = test_db();
    let result = get_scope(&db, "nonexistent.rs", 1);
    assert!(result.is_err());
}
