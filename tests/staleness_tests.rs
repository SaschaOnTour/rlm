//! End-to-end tests for the self-healing index.
//!
//! Verifies that external file changes (anything not routed through
//! `rlm replace` / `rlm insert`) are picked up automatically on the next
//! tool invocation. Also tests the `RLM_SKIP_REFRESH` escape hatch.

#![allow(deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

const INITIAL_SRC: &str = "fn main() {}\n";
const ADDED_SYMBOL: &str = "fn freshly_added_symbol() -> i32 { 42 }\n";
const MODIFIED_SRC: &str = "fn main() { println!(\"edited\"); }\n";

/// Set up an indexed project with a single Rust file.
fn setup_project(initial: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    fs::write(dir.path().join("main.rs"), initial).expect("write main.rs");
    Command::cargo_bin("rlm")
        .unwrap()
        .current_dir(dir.path())
        .arg("index")
        .arg(".")
        .assert()
        .success();
    dir
}

fn rlm(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("rlm").unwrap();
    cmd.current_dir(dir.path());
    cmd
}

#[test]
fn cli_reindexes_after_external_modification() {
    let dir = setup_project(INITIAL_SRC);
    // Overwrite main.rs externally (bypassing rlm replace/insert).
    fs::write(dir.path().join("main.rs"), MODIFIED_SRC).unwrap();

    // A subsequent rlm command should see the new content.
    rlm(&dir)
        .arg("search")
        .arg("edited")
        .assert()
        .success()
        .stdout(predicate::str::contains("edited"));
}

#[test]
fn cli_reindexes_after_external_file_added() {
    let dir = setup_project(INITIAL_SRC);
    // Add a brand-new file externally.
    fs::write(dir.path().join("new_file.rs"), ADDED_SYMBOL).unwrap();

    // The new symbol should be searchable.
    rlm(&dir)
        .arg("search")
        .arg("freshly_added_symbol")
        .assert()
        .success()
        .stdout(predicate::str::contains("freshly_added_symbol"));
}

#[test]
fn cli_reindexes_after_external_file_deleted() {
    let dir = tempfile::tempdir().expect("create tempdir");
    fs::write(dir.path().join("main.rs"), INITIAL_SRC).unwrap();
    fs::write(dir.path().join("doomed.rs"), ADDED_SYMBOL).unwrap();
    Command::cargo_bin("rlm")
        .unwrap()
        .current_dir(dir.path())
        .arg("index")
        .arg(".")
        .assert()
        .success();

    // Delete the file externally.
    fs::remove_file(dir.path().join("doomed.rs")).unwrap();

    // Running a search should trigger staleness cleanup; the orphaned symbol
    // disappears from the index.
    rlm(&dir)
        .arg("search")
        .arg("freshly_added_symbol")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"results\":[]").or(predicate::str::contains("[0]")));
}

#[test]
fn cli_respects_skip_refresh_env() {
    let dir = setup_project(INITIAL_SRC);
    // Add a symbol externally.
    fs::write(dir.path().join("new_file.rs"), ADDED_SYMBOL).unwrap();

    // With RLM_SKIP_REFRESH set, the staleness check is bypassed and the
    // new symbol should NOT appear.
    rlm(&dir)
        .env("RLM_SKIP_REFRESH", "1")
        .arg("search")
        .arg("freshly_added_symbol")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"results\":[]").or(predicate::str::contains("[0]")));

    // Without the env var, it's picked up.
    rlm(&dir)
        .arg("search")
        .arg("freshly_added_symbol")
        .assert()
        .success()
        .stdout(predicate::str::contains("freshly_added_symbol"));
}
