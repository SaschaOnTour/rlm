//! Edge-case tests for `replacer.rs`.
//!
//! Split out of `replacer_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). The happy-path preview test
//! stays in `replacer_tests.rs`; this file covers stale-content detection,
//! same-length tampering, and path-traversal rejection.

use super::{replace_symbol, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

/// End line of the test chunk (3 lines of code).
const CHUNK_END_LINE: u32 = 3;

/// Helper: set up a temp file with content, index it in an in-memory DB,
/// and return `(TempDir, Database, relative-path, project-root)`.
fn setup_temp_project(content: &str) -> (tempfile::TempDir, Database, String, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("lib.rs");
    std::fs::write(&file_path, content).unwrap();

    let db = Database::open_in_memory().unwrap();
    let rel_path = "lib.rs".to_string();
    let f = FileRecord::new(
        rel_path.clone(),
        "h".into(),
        "rust".into(),
        content.len() as u64,
    );
    let fid = db.upsert_file(&f).unwrap();
    let c = Chunk {
        kind: ChunkKind::Function,
        ident: "greet".into(),
        end_line: CHUNK_END_LINE,
        end_byte: content.len() as u32,
        content: content.into(),
        ..Chunk::stub(fid)
    };
    db.insert_chunk(&c).unwrap();

    let project_root = dir.path().to_path_buf();
    (dir, db, rel_path, project_root)
}

#[test]
fn replace_stale_content_rejects() {
    let original = "fn greet() {\n    println!(\"hello\");\n}";
    let (_dir, db, path, root) = setup_temp_project(original);

    // Modify the file on disk after indexing
    std::fs::write(
        root.join(&path),
        "fn greet() {\n    println!(\"goodbye\");\n}",
    )
    .unwrap();

    let result = replace_symbol(&db, &path, "greet", "fn greet() {}", &root);
    assert!(result.is_err(), "should reject stale content");
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("edit conflict"),
        "error should mention edit conflict, got: {msg}"
    );
}

#[test]
fn replace_same_length_different_content_rejects() {
    let original = "fn greet() {\n    println!(\"AAAA\");\n}";
    let (_dir, db, path, root) = setup_temp_project(original);

    let tampered = "fn greet() {\n    println!(\"BBBB\");\n}";
    assert_eq!(
        original.len(),
        tampered.len(),
        "test premise: same byte length"
    );
    std::fs::write(root.join(&path), tampered).unwrap();

    let result = replace_symbol(&db, &path, "greet", "fn greet() {}", &root);
    assert!(
        result.is_err(),
        "should reject same-length different content"
    );
}

#[test]
fn replace_rejects_absolute_path() {
    let db = Database::open_in_memory().unwrap();
    let root = std::path::Path::new("/tmp");
    let result = replace_symbol(&db, "/etc/passwd", "foo", "bar", root);
    assert!(result.is_err());
    assert!(
        format!("{}", result.unwrap_err()).contains("path traversal"),
        "should reject absolute path"
    );
}

#[test]
fn replace_rejects_parent_traversal() {
    let db = Database::open_in_memory().unwrap();
    let root = std::path::Path::new("/tmp");
    let result = replace_symbol(&db, "../etc/passwd", "foo", "bar", root);
    assert!(result.is_err());
    assert!(
        format!("{}", result.unwrap_err()).contains("path traversal"),
        "should reject .. traversal"
    );
}
