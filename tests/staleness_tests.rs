//! End-to-end tests for the self-healing index.
//!
//! Verifies that external file changes (anything not routed through
//! `rlm replace` / `rlm insert`) are picked up automatically on the next
//! tool invocation. Also tests the `RLM_SKIP_REFRESH` escape hatch.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
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

/// Run `rlm search <query>` with `--format json` and return the parsed result
/// count. Asserts the command succeeded and the stdout is valid JSON with a
/// `results` array.
fn search_result_count(dir: &TempDir, query: &str, extra_env: &[(&str, &str)]) -> usize {
    let mut cmd = rlm(dir);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let output = cmd
        .arg("--format")
        .arg("json")
        .arg("search")
        .arg(query)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).expect("search stdout is valid JSON");
    value["results"]
        .as_array()
        .expect("results field is an array")
        .len()
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
    // disappears from the index — parse JSON output to avoid substring brittleness.
    let count = search_result_count(&dir, "freshly_added_symbol", &[]);
    assert_eq!(count, 0, "deleted file's symbol must no longer be indexed");
}

#[cfg(unix)]
#[test]
fn cli_logs_hash_failure_instead_of_silent_drift() {
    // Regression: when staleness can't hash a suspect file (e.g. permission
    // denied), the failure must be logged to stderr — not silently swallowed.
    use std::os::unix::fs::PermissionsExt;

    /// chmod value that denies read access.
    const DENY_READ: u32 = 0o000;
    /// Owner-rw restore value so `TempDir` drop can clean up.
    const RESTORE_RW: u32 = 0o644;

    let dir = setup_project(INITIAL_SRC);
    let file = dir.path().join("main.rs");

    // Force mtime > indexed_at so staleness will attempt to hash.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut content = fs::read(&file).unwrap();
    content.push(b'\n');
    fs::write(&file, &content).unwrap();

    // Revoke read permission so hasher::hash_file fails on File::open.
    fs::set_permissions(&file, fs::Permissions::from_mode(DENY_READ)).unwrap();

    // If permissions can still be bypassed (e.g. tests running as root),
    // the scenario we're asserting can't be constructed — skip the test.
    if fs::read(&file).is_ok() {
        let _ = fs::set_permissions(&file, fs::Permissions::from_mode(RESTORE_RW));
        eprintln!(
            "skipping cli_logs_hash_failure: effective UID bypasses file permissions (root?)"
        );
        return;
    }

    let output = rlm(&dir)
        .arg("search")
        .arg("main")
        .output()
        .expect("command ran");

    // Restore before asserting so TempDir cleanup always works.
    fs::set_permissions(&file, fs::Permissions::from_mode(RESTORE_RW)).unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("staleness hash failed") || stderr.contains("Permission denied"),
        "expected hash-failure log on stderr; got stderr={stderr:?}"
    );
}

#[test]
fn cli_respects_skip_refresh_env() {
    let dir = setup_project(INITIAL_SRC);
    // Add a symbol externally.
    fs::write(dir.path().join("new_file.rs"), ADDED_SYMBOL).unwrap();

    // With RLM_SKIP_REFRESH set, the staleness check is bypassed and the
    // new symbol should NOT appear.
    let count_skip =
        search_result_count(&dir, "freshly_added_symbol", &[("RLM_SKIP_REFRESH", "1")]);
    assert_eq!(
        count_skip, 0,
        "RLM_SKIP_REFRESH=1 must prevent pickup of new symbol"
    );

    // Without the env var, it's picked up.
    rlm(&dir)
        .arg("search")
        .arg("freshly_added_symbol")
        .assert()
        .success()
        .stdout(predicate::str::contains("freshly_added_symbol"));
}
