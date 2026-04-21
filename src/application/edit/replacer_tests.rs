//! Happy-path tests for `replacer.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "replacer_tests.rs"] mod tests;`.
//!
//! Edge-case tests (stale content, same-length tampering, path-traversal)
//! live in the sibling `replacer_edge_tests.rs`.

use super::{preview_replace, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

/// File size in bytes for the test file record.
const TEST_FILE_SIZE: u64 = 100;
/// End line of the test chunk (3 lines of code).
const CHUNK_END_LINE: u32 = 3;
/// End byte offset of the test chunk content "fn main() {\n}".
const CHUNK_END_BYTE: u32 = 14;

#[test]
fn preview_replace_works() {
    let db = Database::open_in_memory().unwrap();
    let f = FileRecord::new("test.rs".into(), "h".into(), "rust".into(), TEST_FILE_SIZE);
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
        content: "fn main() {\n}".into(),
    };
    db.insert_chunk(&c).unwrap();

    let diff = preview_replace(&db, "test.rs", "main", "fn main() { println!(\"hi\"); }").unwrap();
    assert_eq!(diff.symbol, "main");
    assert!(diff.old_code.contains("fn main()"));
}
