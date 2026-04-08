//! End-to-end tests for CLI commands.
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

// ─── rlm index ──────────────────────────────────────────────────────────────

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

// ─── rlm search ─────────────────────────────────────────────────────────────

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

// ─── rlm read ───────────────────────────────────────────────────────────────

#[test]
fn e2e_read_requires_symbol_or_section() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .assert()
        .failure()
        .stderr(predicate::str::contains("symbol").or(predicate::str::contains("section")));
}

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

#[test]
fn e2e_read_metadata() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .arg("--symbol")
        .arg("Config")
        .arg("--metadata")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("chunks").and(
                predicate::str::contains("type_info").or(predicate::str::contains("signature")),
            ),
        );
}

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

// ─── rlm overview ───────────────────────────────────────────────────────────

#[test]
fn e2e_overview_standard() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

#[test]
fn e2e_overview_minimal() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .arg("--detail")
        .arg("minimal")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

#[test]
fn e2e_overview_tree() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .arg("--detail")
        .arg("tree")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"n\":\"sample.rs\"")
                .and(predicate::str::contains("\"dir\":false"))
                .and(predicate::str::contains("\"k\":\"fn\"")),
        );
}

#[test]
fn e2e_overview_with_path_filter() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .arg("--path")
        .arg("sample")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

// ─── rlm refs ───────────────────────────────────────────────────────────────

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

// ─── rlm replace ────────────────────────────────────────────────────────────

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

// ─── rlm insert ─────────────────────────────────────────────────────────────

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

// ─── rlm stats ──────────────────────────────────────────────────────────────

#[test]
fn e2e_stats() {
    let dir = setup_rust_project();
    rlm(&dir).arg("stats").assert().success().stdout(
        predicate::str::contains("\"files\"")
            .and(predicate::str::contains("\"chunks\""))
            .and(predicate::str::contains("\"refs\"")),
    );
}

// ─── rlm partition ──────────────────────────────────────────────────────────

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

// ─── rlm summarize ──────────────────────────────────────────────────────────

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

// ─── rlm diff ───────────────────────────────────────────────────────────────

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

// ─── rlm context ────────────────────────────────────────────────────────────

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

#[test]
fn e2e_context_graph() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("context")
        .arg("helper")
        .arg("--graph")
        .assert()
        .success()
        .stdout(predicate::str::contains("context").and(predicate::str::contains("callgraph")));
}

// ─── rlm deps ───────────────────────────────────────────────────────────────

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

// ─── rlm scope ──────────────────────────────────────────────────────────────

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

// ─── Multi-language indexing ────────────────────────────────────────────────

#[test]
fn e2e_index_multiple_languages() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mdir = manifest_dir();

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

    rlm(&dir)
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("rust").and(predicate::str::contains("go")));
}

// ─── rlm files ──────────────────────────────────────────────────────────────

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
        .stdout(predicate::str::contains("cshtml").not());
}

#[test]
fn e2e_files_path_filter() {
    let dir = setup_mixed_project();

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
        .stdout(predicate::str::contains("\"total\":4"));
}

#[test]
fn e2e_files_no_index_required() {
    let dir = tempfile::tempdir().expect("create tempdir");
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("view.cshtml"), "@model Foo").unwrap();

    rlm(&dir)
        .arg("files")
        .assert()
        .success()
        .stdout(predicate::str::contains("main.rs"))
        .stdout(predicate::str::contains("view.cshtml"));
}

// ─── rlm stats --savings ───────────────────────────────────────────────────

#[test]
fn e2e_stats_savings_empty() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("stats")
        .arg("--savings")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\":0"))
        .stdout(predicate::str::contains("\"saved\":0"));
}

#[test]
fn e2e_stats_savings_after_operations() {
    let dir = setup_rust_project();

    // Run operations that record savings
    rlm(&dir)
        .arg("overview")
        .arg("--detail")
        .arg("minimal")
        .assert()
        .success();
    rlm(&dir).arg("search").arg("Config").assert().success();

    // Savings report should show 2 operations
    rlm(&dir)
        .arg("stats")
        .arg("--savings")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\":2"))
        .stdout(predicate::str::contains("\"by_cmd\""));
}

#[test]
fn e2e_stats_savings_with_since_filter() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .arg("--detail")
        .arg("minimal")
        .assert()
        .success();

    // Future date -> no results
    rlm(&dir)
        .arg("stats")
        .arg("--savings")
        .arg("--since")
        .arg("2099-01-01")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\":0"));

    // Past date -> all results
    rlm(&dir)
        .arg("stats")
        .arg("--savings")
        .arg("--since")
        .arg("2000-01-01")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\":1"));
}
