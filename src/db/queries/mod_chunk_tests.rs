//! Chunk-centric tests for `db::queries::mod`.
//!
//! Split out of `mod_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). File / ref / stats /
//! delete-cascade tests stay in `mod_tests.rs`; this file covers the
//! chunk-level query surface (insert, lookup by id / ident, FTS).

use super::test_fixtures::{sample_chunk, sample_file, test_db, SAMPLE_END_BYTE, SAMPLE_END_LINE};

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
fn get_chunks_by_idents_returns_all_matches() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    db.insert_chunk(&sample_chunk(fid)).unwrap();

    let mut foo = sample_chunk(fid);
    foo.ident = "foo".into();
    foo.start_line = SAMPLE_END_LINE + 1;
    foo.end_line = SAMPLE_END_LINE + SAMPLE_END_LINE;
    foo.start_byte = SAMPLE_END_BYTE + 1;
    foo.end_byte = SAMPLE_END_BYTE + SAMPLE_END_BYTE;
    db.insert_chunk(&foo).unwrap();

    let chunks = db.get_chunks_by_idents(&["main", "foo"]).unwrap();
    let mut idents: Vec<_> = chunks.iter().map(|c| c.ident.clone()).collect();
    idents.sort();
    assert_eq!(idents, vec!["foo", "main"]);
}

#[test]
fn get_chunks_by_idents_empty_input_returns_empty() {
    let db = test_db();
    assert!(db.get_chunks_by_idents(&[]).unwrap().is_empty());
}

#[test]
fn get_chunks_by_idents_unknown_idents_returns_empty() {
    let db = test_db();
    let f = sample_file();
    let fid = db.upsert_file(&f).unwrap();
    db.insert_chunk(&sample_chunk(fid)).unwrap();
    assert!(db
        .get_chunks_by_idents(&["nonexistent"])
        .unwrap()
        .is_empty());
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
