//! Tests for `signature.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "signature_tests.rs"] mod tests;`.

use super::{get_signature, Database};
use crate::domain::chunk::{Chunk, ChunkKind, RefKind, Reference};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_END_LINE_SHORT: u32 = 3;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TEST_END_BYTE_SMALL: u32 = 30;
const TEST_REF_LINE: u32 = 10;
const TEST_REF_COL: u32 = 4;

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn get_signature_basic() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let chunk = Chunk {
        id: 0,
        file_id,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "foo".into(),
        parent: None,
        signature: Some("fn foo(x: i32) -> String".into()),
        visibility: Some("pub".into()),
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "pub fn foo(x: i32) -> String { }".into(),
    };
    let chunk_id = db.insert_chunk(&chunk).unwrap();

    // Add some refs
    let reference = Reference {
        id: 0,
        chunk_id,
        target_ident: "foo".into(),
        ref_kind: RefKind::Call,
        line: TEST_REF_LINE,
        col: TEST_REF_COL,
    };
    db.insert_ref(&reference).unwrap();

    let result = get_signature(&db, "foo", None).unwrap();
    assert_eq!(result.symbol, "foo");
    assert_eq!(result.signatures.len(), 1);
    assert_eq!(result.signatures[0].signature, "fn foo(x: i32) -> String");
    assert!(result.signatures[0].parent.is_none());
    assert_eq!(result.ref_count, 1);
}

// ─── Slice 0.8: parent in signatures ──────────────────────────────────

fn insert_method_with_sig(db: &Database, file_id: i64, ident: &str, parent: &str, sig: &str) {
    let chunk = Chunk {
        kind: ChunkKind::Method,
        ident: ident.into(),
        parent: Some(parent.into()),
        signature: Some(sig.into()),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&chunk).unwrap();
}

#[test]
fn get_signature_includes_parent_for_method() {
    let db = test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method_with_sig(&db, file_id, "make", "Foo", "fn make() -> Self");

    let result = get_signature(&db, "make", None).unwrap();

    assert_eq!(result.signatures.len(), 1);
    assert_eq!(result.signatures[0].parent.as_deref(), Some("Foo"));
    assert_eq!(result.signatures[0].signature, "fn make() -> Self");
}

#[test]
fn get_signature_lists_each_polysemic_definition_with_its_parent() {
    let db = test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method_with_sig(&db, file_id, "new", "Foo", "fn new() -> Self");
    insert_method_with_sig(&db, file_id, "new", "Bar", "fn new(seed: u32) -> Self");

    let result = get_signature(&db, "new", None).unwrap();

    assert_eq!(result.signatures.len(), 2);
    let mut parents: Vec<&str> = result
        .signatures
        .iter()
        .filter_map(|s| s.parent.as_deref())
        .collect();
    parents.sort_unstable();
    assert_eq!(parents, vec!["Bar", "Foo"]);
}

#[test]
fn get_signature_filters_to_parent_when_polysemic() {
    let db = test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method_with_sig(&db, file_id, "new", "Foo", "fn new() -> Self");
    insert_method_with_sig(&db, file_id, "new", "Bar", "fn new(seed: u32) -> Self");

    let result = get_signature(&db, "new", Some("Foo")).unwrap();
    assert_eq!(
        result.signatures.len(),
        1,
        "--parent Foo must drop Bar::new from the signatures list",
    );
    assert_eq!(result.signatures[0].parent.as_deref(), Some("Foo"));
}

#[test]
fn get_signature_empty_when_parent_has_no_match() {
    let db = test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    insert_method_with_sig(&db, file_id, "new", "Foo", "fn new() -> Self");

    let result = get_signature(&db, "new", Some("Bar")).unwrap();
    assert!(
        result.signatures.is_empty(),
        "parent that doesn't match any chunk must yield an empty list",
    );
}

#[test]
fn get_signature_omits_parent_field_for_free_function() {
    let db = test_db();
    let file = FileRecord::new("src/x.rs".into(), "h".into(), "rust".into(), 1);
    let file_id = db.upsert_file(&file).unwrap();
    let chunk = Chunk {
        kind: ChunkKind::Function,
        ident: "helper".into(),
        parent: None,
        signature: Some("fn helper()".into()),
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&chunk).unwrap();

    let result = get_signature(&db, "helper", None).unwrap();

    assert_eq!(result.signatures.len(), 1);
    assert!(result.signatures[0].parent.is_none());
    let json = serde_json::to_string(&result).unwrap();
    assert!(
        !json.contains("\"parent\""),
        "free-fn signatures must not serialise the parent key, got {json}",
    );
}

#[test]
fn get_signature_no_signature() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    let chunk = Chunk {
        id: 0,
        file_id,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE_SHORT,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE_SMALL,
        kind: ChunkKind::Module,
        ident: "mymod".into(),
        parent: None,
        signature: None, // Modules may not have signatures
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "mod mymod {}".into(),
    };
    db.insert_chunk(&chunk).unwrap();

    let result = get_signature(&db, "mymod", None).unwrap();
    assert!(result.signatures.is_empty());
}
