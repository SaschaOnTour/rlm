//! Tests for `summarize.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "summarize_tests.rs"] mod tests;`.

use super::{summarize, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

/// File size in bytes for the test file record.
const TEST_FILE_SIZE: u64 = 500;
/// End line of each test chunk (symbol spans 10 lines).
const CHUNK_END_LINE: u32 = 10;
/// End byte offset of each test chunk.
const CHUNK_END_BYTE: u32 = 100;

#[test]
fn summarize_file() {
    let db = Database::open_in_memory().unwrap();
    let f = FileRecord::new(
        "src/lib.rs".into(),
        "h".into(),
        "rust".into(),
        TEST_FILE_SIZE,
    );
    let fid = db.upsert_file(&f).unwrap();

    for (name, kind, vis) in [
        ("Config", ChunkKind::Struct, "pub"),
        ("new", ChunkKind::Method, "pub"),
        ("helper", ChunkKind::Function, "private"),
    ] {
        db.insert_chunk(&Chunk {
            id: 0,
            file_id: fid,
            start_line: 1,
            end_line: CHUNK_END_LINE,
            start_byte: 0,
            end_byte: CHUNK_END_BYTE,
            kind,
            ident: name.into(),
            parent: None,
            signature: None,
            visibility: Some(vis.into()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "...".into(),
        })
        .unwrap();
    }

    let summary = summarize(&db, "src/lib.rs").unwrap();
    assert_eq!(summary.file, "src/lib.rs");
    assert_eq!(summary.symbols.len(), 3);
    assert!(summary.description.contains("rust"));
}
