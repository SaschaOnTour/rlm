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

    let result = replace_symbol(&db, &path, "greet", None, "fn greet() {}", &root);
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

    let result = replace_symbol(&db, &path, "greet", None, "fn greet() {}", &root);
    assert!(
        result.is_err(),
        "should reject same-length different content"
    );
}

#[test]
fn replace_rejects_absolute_path() {
    let db = Database::open_in_memory().unwrap();
    let root = std::path::Path::new("/tmp");
    let result = replace_symbol(&db, "/etc/passwd", "foo", None, "bar", root);
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
    let result = replace_symbol(&db, "../etc/passwd", "foo", None, "bar", root);
    assert!(result.is_err());
    assert!(
        format!("{}", result.unwrap_err()).contains("path traversal"),
        "should reject .. traversal"
    );
}

// ─── Ambiguous-symbol handling (task #119) ─────────────────────────────

fn setup_two_new_methods() -> (tempfile::TempDir, Database, String, std::path::PathBuf) {
    let source = r#"impl Foo {
    pub fn new() -> Self { Foo }
}
impl Bar {
    pub fn new() -> Self { Bar }
}
"#;
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("lib.rs");
    std::fs::write(&file_path, source).unwrap();

    let db = Database::open_in_memory().unwrap();
    let rel_path = "lib.rs".to_string();
    let f = FileRecord::new(
        rel_path.clone(),
        "h".into(),
        "rust".into(),
        source.len() as u64,
    );
    let fid = db.upsert_file(&f).unwrap();

    let foo_new = "pub fn new() -> Self { Foo }";
    let bar_new = "pub fn new() -> Self { Bar }";
    let foo_start = source.find(foo_new).unwrap() as u32;
    let bar_start = source.find(bar_new).unwrap() as u32;

    let foo = Chunk {
        kind: ChunkKind::Method,
        ident: "new".into(),
        parent: Some("Foo".into()),
        start_line: 2,
        end_line: 2,
        start_byte: foo_start,
        end_byte: foo_start + foo_new.len() as u32,
        content: foo_new.into(),
        ..Chunk::stub(fid)
    };
    let bar = Chunk {
        kind: ChunkKind::Method,
        ident: "new".into(),
        parent: Some("Bar".into()),
        start_line: 5,
        end_line: 5,
        start_byte: bar_start,
        end_byte: bar_start + bar_new.len() as u32,
        content: bar_new.into(),
        ..Chunk::stub(fid)
    };
    db.insert_chunk(&foo).unwrap();
    db.insert_chunk(&bar).unwrap();

    (dir, db, rel_path, dir_to_root(&file_path))
}

fn dir_to_root(file_path: &std::path::Path) -> std::path::PathBuf {
    file_path.parent().unwrap().to_path_buf()
}

#[test]
fn replace_rejects_ambiguous_symbol_without_parent() {
    let (_dir, db, path, root) = setup_two_new_methods();
    let result = super::replace_symbol(&db, &path, "new", None, "whatever", &root);
    assert!(result.is_err(), "ambiguous replace should error");
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("ambiguous symbol 'new'") && msg.contains("Foo") && msg.contains("Bar"),
        "error should list both candidates, got: {msg}"
    );
}

#[test]
fn replace_picks_by_parent_when_ambiguous() {
    let (_dir, db, path, root) = setup_two_new_methods();
    super::replace_symbol(
        &db,
        &path,
        "new",
        Some("Bar"),
        "pub fn new() -> Self { Bar::default() }",
        &root,
    )
    .expect("replace with --parent Bar should succeed");

    let after = std::fs::read_to_string(root.join(&path)).unwrap();
    assert!(
        after.contains("Bar::default()"),
        "Bar's new should have been replaced, got: {after}"
    );
    assert!(
        after.contains("pub fn new() -> Self { Foo }"),
        "Foo's new should be untouched, got: {after}"
    );
}

#[test]
fn replace_errors_on_unknown_parent() {
    let (_dir, db, path, root) = setup_two_new_methods();
    let result = super::replace_symbol(&db, &path, "new", Some("Quux"), "x", &root);
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("Quux") || msg.contains("not found"),
        "error should mention the missing parent, got: {msg}"
    );
}

#[test]
fn replace_unique_symbol_still_works_with_none_parent() {
    // Confirms the new `parent: Option<&str>` parameter is
    // backward-compatible: `None` on an unambiguous symbol still
    // succeeds exactly like before.
    let original = "fn greet() {\n    println!(\"hi\");\n}";
    let (_dir, db, path, root) = setup_temp_project(original);
    super::replace_symbol(&db, &path, "greet", None, "fn greet() {}", &root)
        .expect("unambiguous replace should succeed");
}
