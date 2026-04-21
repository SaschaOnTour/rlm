//! Symbol-diff tests for `diff.rs`.
//!
//! Split out of `diff_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). File-level diff tests
//! stay in `diff_tests.rs`; this file covers the symbol-level variant
//! that requires a chunk in the index.

use super::super::fixtures::setup_test_db_and_dir;
use super::diff_symbol;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;
use std::io::Write;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 3;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;

#[test]
fn diff_symbol_works() {
    let (db, tmp) = setup_test_db_and_dir();

    // Create file on disk
    let file_path = tmp.path().join("test.rs");
    let mut file = std::fs::File::create(&file_path).unwrap();
    writeln!(file, "fn main() {{").unwrap();
    writeln!(file, "    println!(\"hello\");").unwrap();
    writeln!(file, "}}").unwrap();

    // Index the file and chunk
    let file_rec = FileRecord::new(
        "test.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file_rec).unwrap();

    let chunk = Chunk {
        id: 0,
        file_id,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "main".into(),
        parent: None,
        signature: Some("fn main()".into()),
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn main() {\n    println!(\"hello\");\n}".into(),
    };
    db.insert_chunk(&chunk).unwrap();

    let result = diff_symbol(&db, "test.rs", "main", tmp.path()).unwrap();
    assert!(!result.changed);
}
