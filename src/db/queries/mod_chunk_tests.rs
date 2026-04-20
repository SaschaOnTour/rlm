//! Chunk-centric tests for `db::queries::mod`.
//!
//! Split out of `mod_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). File / ref / stats /
//! delete-cascade tests stay in `mod_tests.rs`; this file covers the
//! chunk-level query surface (insert, lookup by id / ident, FTS).

use super::test_fixtures::{sample_chunk, sample_file, test_db};

const FTS_SEARCH_LIMIT: usize = 10;

#[test]
fn insert_chunk_and_retrieve() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    let c = sample_chunk(fid);
    let cid = db.insert_chunk(&c).unwrap();
    assert!(cid > 0);
    let chunks = db.get_chunks_for_file(fid).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].ident, "main");
}

#[test]
fn search_fts_finds_content() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    let c = sample_chunk(fid);
    db.insert_chunk(&c).unwrap();
    let results = db.search_fts("main", FTS_SEARCH_LIMIT).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].ident, "main");
}

#[test]
fn get_chunks_by_ident_works() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    let c = sample_chunk(fid);
    db.insert_chunk(&c).unwrap();
    let chunks = db.get_chunks_by_ident("main").unwrap();
    assert_eq!(chunks.len(), 1);
}

#[test]
fn get_chunk_by_id_returns_inserted_row() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    let c = sample_chunk(fid);
    let cid = db.insert_chunk(&c).unwrap();
    let got = db.get_chunk_by_id(cid).unwrap().expect("chunk by id");
    assert_eq!(got.id, cid);
    assert_eq!(got.ident, "main");
    assert_eq!(got.signature.as_deref(), Some("fn main()"));
}

#[test]
fn get_chunk_by_id_returns_none_for_unknown_id() {
    let db = test_db();
    let got = db.get_chunk_by_id(9_999).unwrap();
    assert!(got.is_none());
}
