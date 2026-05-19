//! Concurrent-reads contract: multiple agents (or shell pipelines)
//! running `rlm read` against the same project must not surface
//! `SQLITE_BUSY` errors.
//!
//! Why this can happen even on the read path: every `rlm read` call
//! goes through `RlmSession::open`, which (a) refreshes staleness —
//! which writes when files moved — and (b) records savings in the
//! background. SQLite's WAL mode permits many concurrent readers but
//! only one writer at a time; without a non-zero `busy_timeout`, the
//! second writer would get `SQLITE_BUSY` immediately. `Database::open`
//! sets `busy_timeout=5000`, which this test exercises end-to-end.
//!
//! The test spawns five processes (not threads) because the realistic
//! scenario is parallel CLI invocations or independent MCP server
//! processes — same lock contention path, but exercised through the
//! same surface users hit.

// Allow deprecated cargo_bin usage until assert_cmd updates API
#![allow(deprecated)]

use assert_cmd::Command;
use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Set up a temp project with the standard Rust fixture and pre-index
/// it once so the concurrent test doesn't race the initial build.
fn setup_indexed_project() -> TempDir {
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

#[test]
fn five_concurrent_reads_never_surface_sqlite_busy() {
    let dir = setup_indexed_project();
    let project_root = dir.path().to_path_buf();

    // Barrier so all five processes fire their `rlm read` as close to
    // simultaneously as the scheduler allows — maximises the window
    // in which they contend for the SQLite writer lock during
    // staleness refresh / savings recording.
    const N: usize = 5;
    let barrier = Arc::new(Barrier::new(N));

    let handles: Vec<_> = (0..N)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let root = project_root.clone();
            thread::spawn(move || {
                barrier.wait();
                Command::cargo_bin("rlm")
                    .unwrap()
                    .current_dir(&root)
                    .arg("read")
                    .arg("sample.rs")
                    .arg("--symbol")
                    .arg("helper")
                    .output()
                    .expect("spawn rlm read")
            })
        })
        .collect();

    let mut failures = Vec::new();
    for (i, h) in handles.into_iter().enumerate() {
        let out = h.join().expect("thread join");
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        if !out.status.success() {
            failures.push(format!(
                "process {i}: exit {:?}, stderr: {stderr}",
                out.status.code(),
            ));
        }
        // Belt-and-braces: even on `success()` exit, a busy collision
        // could in theory leak through as a non-fatal warning. There
        // should be no occurrence of the SQLite busy token anywhere
        // in the output streams.
        for stream in [&stdout, &stderr] {
            assert!(
                !stream.to_ascii_lowercase().contains("sqlite_busy")
                    && !stream.to_ascii_lowercase().contains("database is locked"),
                "process {i} surfaced a busy error:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
            );
        }
    }

    assert!(
        failures.is_empty(),
        "{N} concurrent reads must all succeed; failures:\n  - {}",
        failures.join("\n  - ")
    );
}
