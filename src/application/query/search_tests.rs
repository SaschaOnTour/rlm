//! Tests for `search.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "search_tests.rs"] mod tests;`.

use super::{
    run_fts, sanitize_fts_query, search_chunks, search_chunks_with_fields, Database, FieldsMode,
};
use crate::domain::chunk::{Chunk, ChunkKind};
use crate::domain::file::FileRecord;

const TEST_FILE_BYTES: u64 = 100;
const TEST_START_LINE: u32 = 1;
const TEST_END_LINE: u32 = 5;
const TEST_START_BYTE: u32 = 0;
const TEST_END_BYTE: u32 = 50;
const TEST_SEARCH_LIMIT: usize = 10;

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn search_basic() {
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
        ident: "search_test".into(),
        parent: None,
        signature: Some("fn search_test()".into()),
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: "fn search_test() { println!(\"hello\"); }".into(),
    };
    db.insert_chunk(&chunk).unwrap();

    let result = search_chunks(&db, "search_test", TEST_SEARCH_LIMIT).unwrap();
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].name, "search_test");
    assert_eq!(result.results[0].kind, "fn");
    assert_eq!(result.file_count, 1);
}

#[test]
fn search_no_results() {
    let db = test_db();
    let result = search_chunks(&db, "nonexistent_xyz_123", TEST_SEARCH_LIMIT).unwrap();
    assert!(result.results.is_empty());
    assert_eq!(result.file_count, 0);
}

// ─── sanitize_fts_query contract (post-`docs/bugs/search-sanitizer.md`) ──
//
// The sanitizer used to strip every FTS5 special character and OR-join
// the remaining tokens, which broke phrase + AND queries entirely. The
// new contract:
//   * space-separated tokens pass through unchanged — FTS5 treats that as
//     AND, which is what every other search tool does;
//   * explicit `OR` survives so users can opt into the broader search;
//   * balanced `"..."` phrases survive so FTS5 gets to do phrase matching;
//   * unbalanced trailing `"` is stripped so FTS5 never errors on it;
//   * non-FTS5 punctuation (parens, braces, …) collapses to whitespace
//     (= word break = AND separator).

#[test]
fn sanitize_fts_query_space_is_and() {
    let out = sanitize_fts_query("hello world");
    assert_eq!(out, "hello world");
}

#[test]
fn sanitize_fts_query_explicit_or_survives() {
    let out = sanitize_fts_query("auth OR login");
    assert_eq!(out, "auth OR login");
}

#[test]
fn sanitize_fts_query_balanced_quotes_pass_through() {
    let out = sanitize_fts_query("\"hello world\"");
    assert_eq!(out, "\"hello world\"");
}

#[test]
fn sanitize_fts_query_unbalanced_quote_is_stripped() {
    let out = sanitize_fts_query("hello\"");
    assert!(
        !out.contains('"'),
        "stray quote must be removed, got {out:?}"
    );
    assert_eq!(out.trim(), "hello");
}

#[test]
fn sanitize_fts_query_non_fts_punctuation_becomes_space() {
    let out = sanitize_fts_query("fn main() {}");
    assert_eq!(out, "fn main");
}

#[test]
fn sanitize_fts_query_empty_or_whitespace() {
    assert_eq!(sanitize_fts_query(""), "");
    assert_eq!(sanitize_fts_query("   "), "");
    assert_eq!(sanitize_fts_query("\t\n"), "");
}

// ─── behavioural tests through search_chunks ────────────────────────────

/// Helper: build a minimal `Chunk` record with the given ident + content
/// and insert it into the DB. Returns the assigned chunk id.
fn insert_chunk_with_content(db: &Database, file_id: i64, ident: &str, content: &str) -> i64 {
    let c = Chunk {
        ident: ident.into(),
        content: content.into(),
        kind: ChunkKind::Function,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        ..Chunk::stub(file_id)
    };
    db.insert_chunk(&c).unwrap()
}

fn setup_search_corpus() -> Database {
    let db = test_db();
    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let fid = db.upsert_file(&file).unwrap();
    insert_chunk_with_content(&db, fid, "only_foo", "foo standalone");
    insert_chunk_with_content(&db, fid, "only_bar", "bar standalone");
    insert_chunk_with_content(&db, fid, "foo_and_bar", "foo bar together");
    insert_chunk_with_content(&db, fid, "phrase_exact", "pub enum Command { }");
    insert_chunk_with_content(
        &db,
        fid,
        "words_apart",
        "pub fn main() { let enum_val = Command::new(); }",
    );
    db
}

fn names_of(result: &super::SearchResult) -> Vec<&str> {
    result.results.iter().map(|h| h.name.as_str()).collect()
}

#[test]
fn search_and_semantics_by_default() {
    let db = setup_search_corpus();
    let result = search_chunks(&db, "foo bar", TEST_SEARCH_LIMIT).unwrap();
    let names = names_of(&result);
    assert_eq!(
        names,
        vec!["foo_and_bar"],
        "space-separated tokens must be AND — only chunk with both 'foo' and 'bar' matches"
    );
}

#[test]
fn search_or_explicit_broadens() {
    let db = setup_search_corpus();
    let result = search_chunks(&db, "foo OR bar", TEST_SEARCH_LIMIT).unwrap();
    let names = names_of(&result);
    let set: std::collections::BTreeSet<_> = names.iter().copied().collect();
    assert!(set.contains("only_foo"));
    assert!(set.contains("only_bar"));
    assert!(set.contains("foo_and_bar"));
}

#[test]
fn search_phrase_with_quotes_matches_only_literal() {
    let db = setup_search_corpus();
    let result = search_chunks(&db, "\"pub enum Command\"", TEST_SEARCH_LIMIT).unwrap();
    let names = names_of(&result);
    assert_eq!(
        names,
        vec!["phrase_exact"],
        "quoted phrase must match the contiguous occurrence only, not words_apart"
    );
}

#[test]
fn search_unbalanced_quote_does_not_crash() {
    let db = setup_search_corpus();
    // Unbalanced trailing quote — the sanitizer strips it, FTS5 never sees it.
    let result = search_chunks(&db, "foo\"", TEST_SEARCH_LIMIT);
    assert!(
        result.is_ok(),
        "unbalanced quote must not bubble up as an FTS5 error"
    );
}

#[test]
fn search_non_fts_punctuation_treated_as_and() {
    let db = setup_search_corpus();
    let result = search_chunks(&db, "foo() bar{}", TEST_SEARCH_LIMIT).unwrap();
    let names = names_of(&result);
    assert_eq!(
        names,
        vec!["foo_and_bar"],
        "parens/braces collapse to whitespace → AND semantics"
    );
}

// ─── edge cases ─────────────────────────────────────────────────────────

#[test]
fn search_prefix_star_expands() {
    // FTS5 prefix query: `foo*` matches tokens starting with "foo".
    let db = test_db();
    let file = FileRecord::new(
        "src/lib.rs".into(),
        "h".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let fid = db.upsert_file(&file).unwrap();
    insert_chunk_with_content(&db, fid, "foobar_fn", "foobar standalone");
    insert_chunk_with_content(&db, fid, "foofighter_fn", "foofighter standalone");
    insert_chunk_with_content(&db, fid, "nope_fn", "unrelated word");

    let result = search_chunks(&db, "foo*", TEST_SEARCH_LIMIT).unwrap();
    let names: std::collections::BTreeSet<_> = names_of(&result).into_iter().collect();
    assert!(names.contains("foobar_fn"), "prefix should match foobar");
    assert!(
        names.contains("foofighter_fn"),
        "prefix should match foofighter"
    );
    assert!(
        !names.contains("nope_fn"),
        "prefix should not match unrelated content"
    );
}

#[test]
fn search_unicode_identifier_survives() {
    // Non-ASCII alphanumerics (e.g. `größe`) pass the whitelist and
    // reach FTS5 intact.
    let db = test_db();
    let file = FileRecord::new(
        "src/lib.rs".into(),
        "h".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let fid = db.upsert_file(&file).unwrap();
    insert_chunk_with_content(&db, fid, "groesse_fn", "let größe = 42;");

    let result = search_chunks(&db, "größe", TEST_SEARCH_LIMIT).unwrap();
    let names = names_of(&result);
    assert_eq!(names, vec!["groesse_fn"]);
}

#[test]
fn search_parens_collapse_to_and() {
    // We don't whitelist grouping parens; they become word breaks. The
    // test documents that behaviour so future contributors know
    // parens-in-queries are not respected as FTS5 grouping.
    let db = setup_search_corpus();
    let result = search_chunks(&db, "(foo bar)", TEST_SEARCH_LIMIT).unwrap();
    let names = names_of(&result);
    assert_eq!(
        names,
        vec!["foo_and_bar"],
        "parens strip, space becomes AND → only chunk with both tokens matches"
    );
}

#[test]
fn sanitize_fts_query_multiple_unbalanced_quotes_rebalance() {
    let out = sanitize_fts_query("\"hello \"world");
    let quote_count = out.chars().filter(|&c| c == '"').count();
    assert!(
        quote_count.is_multiple_of(2),
        "balanced quotes required, got {out:?}"
    );
}

#[test]
fn sanitize_fts_query_preserves_underscore_in_identifier() {
    let out = sanitize_fts_query("authenticate_user");
    assert_eq!(out, "authenticate_user");
}

#[test]
fn run_fts_empty_db_returns_empty() {
    let db = test_db();
    let results = run_fts(&db, "hello", TEST_SEARCH_LIMIT).unwrap();
    assert!(results.is_empty());
}

#[test]
fn file_count_deduplicates_hits_in_same_file() {
    let db = test_db();

    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let file_id = db.upsert_file(&file).unwrap();

    // Two distinct chunks in the SAME file, both matching the query.
    for ident in ["foo_alpha", "foo_beta"] {
        let c = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE,
            kind: ChunkKind::Function,
            ident: ident.into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: format!("fn {ident}() {{}}"),
        };
        db.insert_chunk(&c).unwrap();
    }

    let result = search_chunks(&db, "foo", TEST_SEARCH_LIMIT).unwrap();
    assert_eq!(result.results.len(), 2);
    // Two hits in one file → one distinct file.
    assert_eq!(result.file_count, 1);
}

// ─── FieldsMode projection (docs/bugs/search-fields-projection.md) ──────
//
// Default stays `Full` (Scenario C in the bug file: agent gets the code
// immediately, no second call). `Minimal` is the opt-in for agents that
// only need "does this exist?" / "which files?" — content gets dropped
// from each hit, which brings the per-call output from ~5k tokens down
// to a few hundred.

fn setup_single_chunk() -> Database {
    let db = test_db();
    let file = FileRecord::new(
        "src/lib.rs".into(),
        "hash".into(),
        "rust".into(),
        TEST_FILE_BYTES,
    );
    let fid = db.upsert_file(&file).unwrap();
    let c = Chunk {
        file_id: fid,
        start_line: TEST_START_LINE,
        end_line: TEST_END_LINE,
        start_byte: TEST_START_BYTE,
        end_byte: TEST_END_BYTE,
        kind: ChunkKind::Function,
        ident: "auth".into(),
        content: "fn auth() { /* real body */ }".into(),
        ..Chunk::stub(fid)
    };
    db.insert_chunk(&c).unwrap();
    db
}

#[test]
fn search_full_projection_default_includes_content() {
    let db = setup_single_chunk();
    let result = search_chunks(&db, "auth", TEST_SEARCH_LIMIT).unwrap();
    assert_eq!(result.results.len(), 1);
    assert!(
        result.results[0].content.is_some(),
        "default search must include content so scenario C ('one call, code ready') still works"
    );
    assert!(result.results[0]
        .content
        .as_deref()
        .unwrap()
        .contains("real body"));
}

#[test]
fn search_minimal_projection_omits_content() {
    let db = setup_single_chunk();
    let result =
        search_chunks_with_fields(&db, "auth", TEST_SEARCH_LIMIT, FieldsMode::Minimal).unwrap();
    assert_eq!(result.results.len(), 1);
    assert!(
        result.results[0].content.is_none(),
        "--fields minimal must drop the content field, got {:?}",
        result.results[0].content
    );
    // Metadata fields still present
    assert_eq!(result.results[0].name, "auth");
    assert_eq!(result.results[0].kind, "fn");
    assert_eq!(result.results[0].lines, (TEST_START_LINE, TEST_END_LINE));
}

#[test]
fn search_full_projection_explicit_matches_default() {
    let db = setup_single_chunk();
    let explicit =
        search_chunks_with_fields(&db, "auth", TEST_SEARCH_LIMIT, FieldsMode::Full).unwrap();
    let implicit = search_chunks(&db, "auth", TEST_SEARCH_LIMIT).unwrap();
    // Same name + lines + content — explicit Full is the same as the convenience wrapper.
    assert_eq!(explicit.results.len(), implicit.results.len());
    assert_eq!(explicit.results[0].content, implicit.results[0].content);
}

#[test]
fn search_minimal_result_serialises_without_content_key() {
    let db = setup_single_chunk();
    let result =
        search_chunks_with_fields(&db, "auth", TEST_SEARCH_LIMIT, FieldsMode::Minimal).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    assert!(
        !json.contains("\"content\""),
        "minimal projection must not serialise the content key, got {json}"
    );
    // Metadata keys are still there.
    assert!(json.contains("\"name\":\"auth\""));
    assert!(json.contains("\"kind\":\"fn\""));
}

#[test]
fn search_no_hits_minimal_still_serialises_empty_results() {
    let db = test_db();
    let result =
        search_chunks_with_fields(&db, "nonexistent", TEST_SEARCH_LIMIT, FieldsMode::Minimal)
            .unwrap();
    assert!(result.results.is_empty());
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"results\":[]"));
}
