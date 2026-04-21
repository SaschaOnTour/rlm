//! Tests for `tree.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "tree_tests.rs"] mod tests;`.

use super::{build_tree, format_tree, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

/// File size in bytes for the test file record.
const TEST_FILE_SIZE: u64 = 100;
/// End line of the test chunk.
const CHUNK_END_LINE: u32 = 3;
/// End byte offset of the test chunk content "fn main() {}".
const CHUNK_END_BYTE: u32 = 30;

#[test]
fn build_tree_from_db() {
    let db = Database::open_in_memory().unwrap();
    let f1 = FileRecord::new(
        "src/main.rs".into(),
        "h1".into(),
        "rust".into(),
        TEST_FILE_SIZE,
    );
    let fid = db.upsert_file(&f1).unwrap();
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
        signature: Some("fn main()".into()),
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn main() {}".into(),
    };
    db.insert_chunk(&c).unwrap();

    let tree = build_tree(&db, None).unwrap();
    assert!(!tree.results.is_empty());
    let formatted = format_tree(&tree.results, 0);
    assert!(formatted.contains("src/"));
    assert!(formatted.contains("main.rs"));
    assert!(formatted.contains("fn:main"));

    // Verify structured JSON serialization
    let json = serde_json::to_string(&tree).unwrap();
    assert!(json.contains("\"name\":"), "should have key 'name'");
    assert!(json.contains("\"children\":"), "should have key 'children'");
    assert!(json.contains("\"symbols\":"), "should have key 'symbols'");
    assert!(json.contains("\"is_dir\":"), "should have 'is_dir' key");
    assert!(
        json.contains("\"kind\":"),
        "should have key 'kind' for symbol kind"
    );
    assert!(json.contains("\"line\":"), "should have key 'line'");
}

#[test]
fn format_tree_empty() {
    let formatted = format_tree(&[], 0);
    assert!(formatted.is_empty());
}
