//! Shared fixtures for the `db::queries` companion test files.
//!
//! Both `mod_tests.rs` and `mod_chunk_tests.rs` construct the same
//! `FileRecord` + `Chunk` sample; centralising them here keeps the
//! two files free of duplicate helpers (BP / DRY).

use crate::db::Database;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

pub(super) const SAMPLE_FILE_SIZE: u64 = 1024;
pub(super) const SAMPLE_END_LINE: u32 = 10;
pub(super) const SAMPLE_END_BYTE: u32 = 200;

pub(super) fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

pub(super) fn sample_file() -> FileRecord {
    FileRecord::new(
        "src/main.rs".into(),
        "abc123".into(),
        "rust".into(),
        SAMPLE_FILE_SIZE,
    )
}

pub(super) fn sample_chunk(file_id: i64) -> Chunk {
    Chunk {
        id: 0,
        file_id,
        start_line: 1,
        end_line: SAMPLE_END_LINE,
        start_byte: 0,
        end_byte: SAMPLE_END_BYTE,
        kind: ChunkKind::Function,
        ident: "main".into(),
        parent: None,
        signature: Some("fn main()".into()),
        visibility: Some("pub".into()),
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn main() {\n    println!(\"hello\");\n}".into(),
    }
}
