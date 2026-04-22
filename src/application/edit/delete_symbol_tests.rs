//! Tests for `delete_symbol` (T3→T4 dogfood feature).
//!
//! Mirrors `replacer_edge_tests.rs` layout — an in-memory DB paired with a
//! TempDir file. Covers: happy-path body removal, trailing-newline
//! collapse, stale-chunk rejection, unknown-symbol error, and Syntax
//! Guard rejection when the remaining file would not parse.

use super::{delete_symbol, Database};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

fn setup_with(
    content: &str,
    ident: &str,
    start_byte: u32,
    end_byte: u32,
    chunk_content: &str,
) -> (tempfile::TempDir, Database, String, std::path::PathBuf) {
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
        ident: ident.into(),
        start_line: 1,
        end_line: 3,
        start_byte,
        end_byte,
        content: chunk_content.into(),
        ..Chunk::stub(fid)
    };
    db.insert_chunk(&c).unwrap();

    let project_root = dir.path().to_path_buf();
    (dir, db, rel_path, project_root)
}

#[test]
fn delete_symbol_removes_function_body_and_trailing_newline() {
    // A tiny two-function file. Delete the first; expect the second to
    // become the only function left, with no orphaned blank line.
    let content = "fn greet() {\n    println!(\"hi\");\n}\nfn farewell() {}\n";
    let greet = "fn greet() {\n    println!(\"hi\");\n}";
    let start = 0_u32;
    let end = greet.len() as u32;
    let (_dir, db, path, root) = setup_with(content, "greet", start, end, greet);

    delete_symbol(&db, &path, "greet", None, false, &root).expect("delete should succeed");

    let after = std::fs::read_to_string(root.join(&path)).unwrap();
    // Remaining file is just the second function (trailing newline from
    // the original preserved).
    assert_eq!(after, "fn farewell() {}\n");
}

#[test]
fn delete_symbol_rejects_unknown_symbol() {
    let content = "fn greet() {}\n";
    let (_dir, db, path, root) = setup_with(content, "greet", 0, 13, "fn greet() {}");

    let result = delete_symbol(&db, &path, "nonexistent", None, false, &root);
    assert!(result.is_err(), "unknown symbol should error");
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("not found") || msg.contains("nonexistent"),
        "error should identify the missing symbol, got: {msg}"
    );
}

#[test]
fn delete_symbol_rejects_stale_chunk() {
    // File on disk drifted from indexed byte range — reject with
    // EditConflict, same as replace.
    let original = "fn greet() {\n    println!(\"hi\");\n}";
    let (_dir, db, path, root) = setup_with(original, "greet", 0, original.len() as u32, original);

    std::fs::write(
        root.join(&path),
        "fn greet() {\n    println!(\"goodbye\");\n}",
    )
    .unwrap();

    let result = delete_symbol(&db, &path, "greet", None, false, &root);
    assert!(result.is_err(), "stale chunk should be rejected");
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("edit conflict"),
        "error should mention edit conflict, got: {msg}"
    );
}

#[test]
fn delete_symbol_removes_last_symbol_leaving_minimal_whitespace() {
    // Deleting the only symbol in a file leaves a whitespace-only file.
    // That's still parseable Rust (empty module), so Syntax Guard must
    // accept it.
    let content = "fn greet() {}\n";
    let (_dir, db, path, root) = setup_with(content, "greet", 0, 13, "fn greet() {}");

    delete_symbol(&db, &path, "greet", None, false, &root).expect("delete should succeed");

    let after = std::fs::read_to_string(root.join(&path)).unwrap();
    // Either entirely empty or just a leftover newline — both acceptable.
    assert!(
        after.trim().is_empty(),
        "file should be empty after deleting the last symbol, got: {after:?}"
    );
}

#[test]
fn delete_symbol_syntax_guard_rejects_if_remaining_file_invalid() {
    // The file has a syntactically broken tail that only parses because
    // `greet`'s braces balance the tail's construct. Removing `greet`
    // leaves unbalanced braces in the file — Syntax Guard must reject.
    // We synthesise this by giving the post-delete file a stray `}`.
    let content = "fn greet() {}\n}\n";
    let (_dir, db, path, root) = setup_with(content, "greet", 0, 13, "fn greet() {}");

    let result = delete_symbol(&db, &path, "greet", None, false, &root);
    assert!(
        result.is_err(),
        "Syntax Guard should reject post-delete invalid file"
    );
}

// ─── Doc-comment + attribute sidecar deletion (task #120) ──────────

fn setup_with_sidecar(
    content: &str,
    ident: &str,
    chunk_start: u32,
    chunk_end: u32,
    chunk_content: &str,
) -> (tempfile::TempDir, Database, String, std::path::PathBuf) {
    setup_with(content, ident, chunk_start, chunk_end, chunk_content)
}

#[test]
fn delete_removes_doc_comment_by_default() {
    let content = "/// Does nothing useful.\npub fn stub() {}\n";
    let body = "pub fn stub() {}";
    let body_start = content.find(body).unwrap() as u32;
    let (_dir, db, path, root) = setup_with_sidecar(
        content,
        "stub",
        body_start,
        body_start + body.len() as u32,
        body,
    );

    let outcome = delete_symbol(&db, &path, "stub", None, false, &root)
        .expect("default delete should succeed");

    let after = std::fs::read_to_string(root.join(&path)).unwrap();
    assert!(
        !after.contains("Does nothing useful"),
        "doc comment should have been removed, got: {after:?}"
    );
    assert!(
        after.trim().is_empty(),
        "file should be empty, got: {after:?}"
    );
    assert!(
        outcome.sidecar_lines.is_some(),
        "DeleteOutcome should record the sidecar line range"
    );
}

#[test]
fn delete_removes_attribute_by_default() {
    let content = "#[deprecated]\npub fn old() {}\n";
    let body = "pub fn old() {}";
    let body_start = content.find(body).unwrap() as u32;
    let (_dir, db, path, root) = setup_with_sidecar(
        content,
        "old",
        body_start,
        body_start + body.len() as u32,
        body,
    );

    delete_symbol(&db, &path, "old", None, false, &root).expect("delete");

    let after = std::fs::read_to_string(root.join(&path)).unwrap();
    assert!(
        !after.contains("#[deprecated]"),
        "attribute should have been removed, got: {after:?}"
    );
}

#[test]
fn delete_removes_doc_and_attr_together() {
    let content = "/// Deprecated helper.\n#[deprecated]\npub fn combo() {}\n";
    let body = "pub fn combo() {}";
    let body_start = content.find(body).unwrap() as u32;
    let (_dir, db, path, root) = setup_with_sidecar(
        content,
        "combo",
        body_start,
        body_start + body.len() as u32,
        body,
    );

    delete_symbol(&db, &path, "combo", None, false, &root).expect("delete");

    let after = std::fs::read_to_string(root.join(&path)).unwrap();
    assert!(
        after.trim().is_empty(),
        "all three lines should be gone: {after:?}"
    );
}

#[test]
fn delete_keep_docs_preserves_sidecar() {
    let content = "/// Keep me.\n#[deprecated]\npub fn replaceable() {}\n";
    let body = "pub fn replaceable() {}";
    let body_start = content.find(body).unwrap() as u32;
    let (_dir, db, path, root) = setup_with_sidecar(
        content,
        "replaceable",
        body_start,
        body_start + body.len() as u32,
        body,
    );

    let outcome = delete_symbol(&db, &path, "replaceable", None, true, &root).expect("delete");

    let after = std::fs::read_to_string(root.join(&path)).unwrap();
    assert!(
        after.contains("/// Keep me."),
        "doc should stay with --keep-docs, got: {after:?}"
    );
    assert!(
        after.contains("#[deprecated]"),
        "attr should stay with --keep-docs, got: {after:?}"
    );
    assert!(
        outcome.sidecar_lines.is_none(),
        "no sidecar removed → no range reported"
    );
}

#[test]
fn delete_stops_sidecar_extension_at_blank_line() {
    // Blank line between doc and symbol → they're conceptually
    // separate (maybe the doc belongs to the previous item or is a
    // floating comment). Delete should only take the symbol.
    let content = "/// Section header.\n\npub fn lonely() {}\n";
    let body = "pub fn lonely() {}";
    let body_start = content.find(body).unwrap() as u32;
    let (_dir, db, path, root) = setup_with_sidecar(
        content,
        "lonely",
        body_start,
        body_start + body.len() as u32,
        body,
    );

    delete_symbol(&db, &path, "lonely", None, false, &root).expect("delete");

    let after = std::fs::read_to_string(root.join(&path)).unwrap();
    assert!(
        after.contains("/// Section header."),
        "floating doc above blank line should be preserved, got: {after:?}"
    );
}
