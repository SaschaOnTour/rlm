//! End-to-end tests for all 24 CLI commands.
//!
//! Each test:
//! 1. Creates a temp directory
//! 2. Copies fixture files into it
//! 3. Runs `rlm index .`
//! 4. Runs the specific command
//! 5. Asserts exit code 0 + expected output

// Allow deprecated cargo_bin usage until assert_cmd updates API
#![allow(deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Manifest directory (project root).
fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Copy the Rust fixture into a temp directory and index it.
fn setup_rust_project() -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    let fixture = format!("{}/fixtures/code_samples/rust/sample.rs", manifest_dir());
    fs::copy(&fixture, dir.path().join("sample.rs")).expect("copy fixture");

    Command::cargo_bin("rlm")
        .unwrap()
        .current_dir(dir.path())
        .arg("index")
        .arg(".")
        .assert()
        .success();

    dir
}

/// Copy the markdown fixture into a temp directory and index it.
fn setup_markdown_project() -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    let fixture = format!("{}/fixtures/markdown/sample.md", manifest_dir());
    fs::copy(&fixture, dir.path().join("sample.md")).expect("copy fixture");

    Command::cargo_bin("rlm")
        .unwrap()
        .current_dir(dir.path())
        .arg("index")
        .arg(".")
        .assert()
        .success();

    dir
}

/// Build a command pointing at the tempdir.
fn rlm(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("rlm").unwrap();
    cmd.current_dir(dir.path());
    cmd
}

// ─── 1. rlm index ───────────────────────────────────────────────────────────

#[test]
fn e2e_index_creates_db() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let fixture = format!("{}/fixtures/code_samples/rust/sample.rs", manifest_dir());
    fs::copy(&fixture, dir.path().join("sample.rs")).expect("copy fixture");

    rlm(&dir)
        .arg("index")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("files_indexed"));

    assert!(dir.path().join(".rlm/index.db").exists());
}

// ─── 2. rlm search ──────────────────────────────────────────────────────────

#[test]
fn e2e_search_finds_config() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("search")
        .arg("Config")
        .assert()
        .success()
        .stdout(predicate::str::contains("Config"));
}

// ─── 3. rlm read <path> ─────────────────────────────────────────────────────

#[test]
fn e2e_read_file() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .assert()
        .success()
        .stdout(predicate::str::contains("pub struct Config"));
}

// ─── 4. rlm read <path> --symbol <name> ─────────────────────────────────────

#[test]
fn e2e_read_symbol() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .arg("--symbol")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"));
}

// ─── 5. rlm tree ────────────────────────────────────────────────────────────

#[test]
fn e2e_tree() {
    let dir = setup_rust_project();
    rlm(&dir).arg("tree").assert().success().stdout(
        predicate::str::contains("\"n\":\"sample.rs\"")
            .and(predicate::str::contains("\"dir\":false"))
            .and(predicate::str::contains("\"k\":\"fn\"")),
    );
}

// ─── 6. rlm refs <symbol> ───────────────────────────────────────────────────

#[test]
fn e2e_refs() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("refs")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"s\":\"helper\""));
}

// ─── 7. rlm signature <symbol> ──────────────────────────────────────────────

#[test]
fn e2e_signature() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("signature")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"s\":\"helper\""));
}

// ─── 8. rlm replace --preview ───────────────────────────────────────────────

#[test]
fn e2e_replace_preview() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("replace")
        .arg("sample.rs")
        .arg("--symbol")
        .arg("helper")
        .arg("--code")
        .arg("pub fn helper(x: i32) -> i32 {\n    x * 3\n}")
        .arg("--preview")
        .assert()
        .success()
        .stdout(predicate::str::contains("old_code").and(predicate::str::contains("new_code")));

    // Verify file was NOT modified (preview only)
    let content = fs::read_to_string(dir.path().join("sample.rs")).unwrap();
    assert!(
        content.contains("x * 2"),
        "file should not be modified in preview mode"
    );
}

// ─── 9. rlm insert ──────────────────────────────────────────────────────────

#[test]
fn e2e_insert_top() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("insert")
        .arg("sample.rs")
        .arg("--code")
        .arg("// inserted at top\n")
        .arg("--position")
        .arg("top")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"));

    let content = fs::read_to_string(dir.path().join("sample.rs")).unwrap();
    assert!(content.starts_with("// inserted at top"));
}

// ─── 10. rlm stats ──────────────────────────────────────────────────────────

#[test]
fn e2e_stats() {
    let dir = setup_rust_project();
    rlm(&dir).arg("stats").assert().success().stdout(
        predicate::str::contains("\"files\"")
            .and(predicate::str::contains("\"chunks\""))
            .and(predicate::str::contains("\"refs\"")),
    );
}

// ─── 11. rlm peek ───────────────────────────────────────────────────────────

#[test]
fn e2e_peek() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("peek")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

// ─── 12. rlm grep ───────────────────────────────────────────────────────────

#[test]
fn e2e_grep() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("grep")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"));
}

// ─── 13. rlm partition ──────────────────────────────────────────────────────

#[test]
fn e2e_partition_uniform() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("partition")
        .arg("sample.rs")
        .arg("--strategy")
        .arg("uniform:10")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ─── 14. rlm summarize ──────────────────────────────────────────────────────

#[test]
fn e2e_summarize() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("summarize")
        .arg("sample.rs")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ─── 15. rlm batch ──────────────────────────────────────────────────────────

#[test]
fn e2e_batch() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("batch")
        .arg("Config")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ─── 16. rlm diff ───────────────────────────────────────────────────────────

#[test]
fn e2e_diff_unchanged() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("diff")
        .arg("sample.rs")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"changed\":false"));
}

// ─── 17. rlm map ────────────────────────────────────────────────────────────

#[test]
fn e2e_map() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("map")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

// ─── 18. rlm callgraph ──────────────────────────────────────────────────────

#[test]
fn e2e_callgraph() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("callgraph")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"s\":\"helper\""));
}

// ─── 19. rlm impact ─────────────────────────────────────────────────────────

#[test]
fn e2e_impact() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("impact")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"s\":\"helper\""));
}

// ─── 20. rlm context ────────────────────────────────────────────────────────

#[test]
fn e2e_context() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("context")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"s\":\"helper\"").and(predicate::str::contains("body")));
}

// ─── 21. rlm deps ───────────────────────────────────────────────────────────

#[test]
fn e2e_deps() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("deps")
        .arg("sample.rs")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"f\":\"sample.rs\""));
}

// ─── 22. rlm scope ──────────────────────────────────────────────────────────

#[test]
fn e2e_scope() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("scope")
        .arg("sample.rs")
        .arg("--line")
        .arg("10")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"f\":\"sample.rs\""));
}

// ─── 23. rlm type ───────────────────────────────────────────────────────────

#[test]
fn e2e_type() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("type")
        .arg("Config")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"s\":\"Config\""));
}

// ─── 24. rlm patterns ───────────────────────────────────────────────────────

#[test]
fn e2e_patterns() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("patterns")
        .arg("Config")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"q\":\"Config\""));
}

// ─── Markdown-specific tests ─────────────────────────────────────────────────

#[test]
fn e2e_read_markdown_section() {
    let dir = setup_markdown_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.md")
        .arg("--section")
        .arg("Installation")
        .assert()
        .success()
        .stdout(predicate::str::contains("install"));
}

// ─── Read with line range ────────────────────────────────────────────────────

#[test]
fn e2e_read_line_range() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .arg("--lines")
        .arg("1-5")
        .assert()
        .success()
        .stdout(predicate::str::contains("struct Config"));
}

// ─── Partition semantic strategy ─────────────────────────────────────────────

#[test]
fn e2e_partition_semantic() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("partition")
        .arg("sample.rs")
        .arg("--strategy")
        .arg("semantic")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ─── Diff with symbol ───────────────────────────────────────────────────────

#[test]
fn e2e_diff_symbol() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("diff")
        .arg("sample.rs")
        .arg("--symbol")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"changed\":false"));
}

// ─── Grep with context lines ────────────────────────────────────────────────

#[test]
fn e2e_grep_with_context() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("grep")
        .arg("Config")
        .arg("--context")
        .arg("2")
        .assert()
        .success()
        .stdout(predicate::str::contains("Config"));
}

// ─── Search with limit ──────────────────────────────────────────────────────

#[test]
fn e2e_search_with_limit() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("search")
        .arg("fn")
        .arg("--limit")
        .arg("2")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ─── Map with path filter ───────────────────────────────────────────────────

#[test]
fn e2e_map_with_path_filter() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("map")
        .arg("sample")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

// ─── Peek with path filter ──────────────────────────────────────────────────

#[test]
fn e2e_peek_with_path_filter() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("peek")
        .arg("sample")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

// ─── Multi-language indexing ─────────────────────────────────────────────────

#[test]
fn e2e_index_multiple_languages() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mdir = manifest_dir();

    // Copy fixtures for multiple languages
    fs::copy(
        format!("{mdir}/fixtures/code_samples/rust/sample.rs"),
        dir.path().join("sample.rs"),
    )
    .unwrap();
    fs::copy(
        format!("{mdir}/fixtures/code_samples/go/sample.go"),
        dir.path().join("sample.go"),
    )
    .unwrap();
    fs::copy(
        format!("{mdir}/fixtures/code_samples/python/sample.py"),
        dir.path().join("sample.py"),
    )
    .unwrap();

    rlm(&dir)
        .arg("index")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("files_indexed"));

    // Verify stats shows multiple languages
    rlm(&dir)
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("rust").and(predicate::str::contains("go")));
}

// ─── rlm files ─────────────────────────────────────────────────────────────

/// Setup with mixed file types (supported + unsupported)
fn setup_mixed_project() -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");

    // Supported files
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn lib() {}").unwrap();

    // Unsupported files
    fs::write(dir.path().join("view.cshtml"), "@model Foo").unwrap();
    fs::write(dir.path().join("app.kt"), "fun main() {}").unwrap();

    // Index (only .rs files will be indexed)
    Command::cargo_bin("rlm")
        .unwrap()
        .current_dir(dir.path())
        .arg("index")
        .arg(".")
        .assert()
        .success();

    dir
}

#[test]
fn e2e_files_shows_all() {
    let dir = setup_mixed_project();
    rlm(&dir)
        .arg("files")
        .assert()
        .success()
        .stdout(predicate::str::contains("main.rs"))
        .stdout(predicate::str::contains("view.cshtml"))
        .stdout(predicate::str::contains("app.kt"));
}

#[test]
fn e2e_files_skipped_only() {
    let dir = setup_mixed_project();
    rlm(&dir)
        .arg("files")
        .arg("--skipped-only")
        .assert()
        .success()
        .stdout(predicate::str::contains("cshtml"))
        .stdout(predicate::str::contains("kt"))
        // Should show i:false for skipped files
        .stdout(predicate::str::contains("\"i\":false"));
}

#[test]
fn e2e_files_indexed_only() {
    let dir = setup_mixed_project();
    rlm(&dir)
        .arg("files")
        .arg("--indexed-only")
        .assert()
        .success()
        .stdout(predicate::str::contains(".rs"))
        // Should NOT contain the unsupported files
        .stdout(predicate::str::contains("cshtml").not());
}

#[test]
fn e2e_files_path_filter() {
    let dir = setup_mixed_project();

    // Create subdirectory with files
    fs::create_dir(dir.path().join("views")).unwrap();
    fs::write(dir.path().join("views/page.cshtml"), "@model Page").unwrap();

    rlm(&dir)
        .arg("files")
        .arg("--path")
        .arg("views/")
        .assert()
        .success()
        .stdout(predicate::str::contains("page.cshtml"))
        .stdout(predicate::str::contains("main.rs").not());
}

#[test]
fn e2e_files_summary_counts() {
    let dir = setup_mixed_project();
    rlm(&dir)
        .arg("files")
        .assert()
        .success()
        // Should have summary with total count
        .stdout(predicate::str::contains("\"total\":4"));
}

#[test]
fn e2e_files_no_index_required() {
    // files command should work even without prior indexing
    let dir = tempfile::tempdir().expect("create tempdir");
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("view.cshtml"), "@model Foo").unwrap();

    // Don't run index!
    rlm(&dir)
        .arg("files")
        .assert()
        .success()
        .stdout(predicate::str::contains("main.rs"))
        .stdout(predicate::str::contains("view.cshtml"));
}
