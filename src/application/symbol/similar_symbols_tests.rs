//! Tests for `similar_symbols.rs` (task #106 / T5).

use super::{find_similar_symbols, levenshtein};
use crate::db::Database;
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

fn setup_db(files_and_idents: &[(&str, Vec<(&str, ChunkKind)>)]) -> Database {
    let db = Database::open_in_memory().unwrap();
    for (path, chunks) in files_and_idents {
        let f = FileRecord::new(path.to_string(), "h".into(), "rust".into(), 100);
        let fid = db.upsert_file(&f).unwrap();
        for (ident, kind) in chunks {
            let c = Chunk {
                kind: kind.clone(),
                ident: ident.to_string(),
                ..Chunk::stub(fid)
            };
            db.insert_chunk(&c).unwrap();
        }
    }
    db
}

// ─── levenshtein ───────────────────────────────────────────────────────

#[test]
fn levenshtein_identical_is_zero() {
    assert_eq!(levenshtein("abc", "abc"), 0);
    assert_eq!(levenshtein("", ""), 0);
}

#[test]
fn levenshtein_empty_vs_nonempty_is_length() {
    assert_eq!(levenshtein("", "abc"), 3);
    assert_eq!(levenshtein("abcd", ""), 4);
}

#[test]
fn levenshtein_single_edit() {
    // substitution
    assert_eq!(levenshtein("cat", "bat"), 1);
    // insertion
    assert_eq!(levenshtein("cat", "cats"), 1);
    // deletion
    assert_eq!(levenshtein("cats", "cat"), 1);
}

#[test]
fn levenshtein_multiple_edits() {
    // "opne" → "open" = 2 transpositions (two substitutions in DP
    // terms, since we don't have a swap op).
    assert_eq!(levenshtein("opne", "open"), 2);
    assert_eq!(levenshtein("kitten", "sitting"), 3);
}

#[test]
fn levenshtein_unicode_counts_chars_not_bytes() {
    // "ä" is 2 bytes, one char. The distance must count chars so
    // ASCII and non-ASCII idents are treated consistently.
    assert_eq!(levenshtein("a", "ä"), 1);
    assert_eq!(levenshtein("café", "cafe"), 1);
}

// ─── find_similar_symbols ──────────────────────────────────────────────

#[test]
fn find_similar_excludes_same_file() {
    let db = setup_db(&[
        ("src/auth.rs", vec![("authenticate", ChunkKind::Function)]),
        (
            "src/helpers.rs",
            vec![
                ("authenicate", ChunkKind::Function), // typo, distance 1
                ("authorize", ChunkKind::Function),   // distance 4 — out of range
            ],
        ),
    ]);
    let auth_fid = db.get_file_by_path("src/auth.rs").unwrap().unwrap().id;

    let hits = find_similar_symbols(&db, "authenticate", Some(auth_fid), 3, 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].symbol, "authenicate");
    assert_eq!(hits[0].file, "src/helpers.rs");
    assert_eq!(hits[0].distance, 1);
}

#[test]
fn find_similar_respects_distance_ceiling() {
    let db = setup_db(&[(
        "src/lib.rs",
        vec![
            ("login", ChunkKind::Function),
            ("logout", ChunkKind::Function), // dist 2 from login
            ("logs_session", ChunkKind::Function), // dist 7 from login
        ],
    )]);
    // No exclusion (looking for neighbours of a fictive `login` from outside).
    let hits = find_similar_symbols(&db, "login", None, 3, 5).unwrap();
    let idents: Vec<&str> = hits.iter().map(|h| h.symbol.as_str()).collect();
    assert!(idents.contains(&"logout"), "got: {idents:?}");
    assert!(!idents.contains(&"logs_session"));
    assert!(
        !idents.contains(&"login"),
        "exact match excluded, got: {idents:?}"
    );
}

#[test]
fn find_similar_caps_at_top_n() {
    let chunks: Vec<(&str, ChunkKind)> = [
        "parse", "parse1", "parse2", "parse3", "parse4", "parse5", "parse6",
    ]
    .iter()
    .map(|s| (*s, ChunkKind::Function))
    .collect();
    let db = setup_db(&[("src/p.rs", chunks)]);
    let hits = find_similar_symbols(&db, "parse", None, 3, 3).unwrap();
    assert_eq!(hits.len(), 3, "top_n cap enforced");
}

#[test]
fn find_similar_orders_by_distance_then_name() {
    let db = setup_db(&[(
        "src/f.rs",
        vec![
            ("apply", ChunkKind::Function),  // dist 2
            ("appled", ChunkKind::Function), // dist 1
            ("appleb", ChunkKind::Function), // dist 1 — alphabetical tie-break before "appled"
        ],
    )]);
    let hits = find_similar_symbols(&db, "apple", None, 3, 5).unwrap();
    let symbols: Vec<&str> = hits.iter().map(|h| h.symbol.as_str()).collect();
    assert_eq!(
        symbols,
        vec!["appleb", "appled", "apply"],
        "stable sort: distance first, then alphabetical"
    );
}

#[test]
fn find_similar_ignores_non_function_kinds() {
    let db = setup_db(&[(
        "src/f.rs",
        vec![
            ("UserConfig", ChunkKind::Struct),    // struct, not a fn → ignore
            ("user_config", ChunkKind::Function), // fn, similar → keep
        ],
    )]);
    let hits = find_similar_symbols(&db, "user_conig", None, 3, 5).unwrap();
    let symbols: Vec<&str> = hits.iter().map(|h| h.symbol.as_str()).collect();
    assert_eq!(symbols, vec!["user_config"]);
}

#[test]
fn find_similar_empty_target_returns_empty() {
    let db = setup_db(&[("src/f.rs", vec![("anything", ChunkKind::Function)])]);
    let hits = find_similar_symbols(&db, "", None, 3, 5).unwrap();
    assert!(hits.is_empty());
}
