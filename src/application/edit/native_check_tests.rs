//! Tests for `native_check.rs` (task #115).
//!
//! These invoke real `cargo check` subprocesses against tiny tempdir
//! projects. They are slow(ish) — typically 1-3s each on incremental
//! runs — so the surface is kept small: one positive, one
//! syntax-failure, one name-resolution failure, plus guards against
//! running without a Cargo.toml and when disabled via config.

use super::{run_check, BuildReport};
use crate::config::EditSettings;
use std::fs;
use tempfile::TempDir;

/// Set up a minimal Cargo project with the given `lib.rs` content.
/// The Cargo.toml has no dependencies so `cargo check` stays fast and
/// fully offline.
fn setup_cargo_project(lib_rs: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "rlm_native_check_probe"
version = "0.0.1"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), lib_rs).unwrap();
    dir
}

fn default_settings() -> EditSettings {
    EditSettings {
        native_check: true,
        native_check_timeout_secs: 30,
    }
}

#[test]
fn rust_check_passes_on_valid_code() {
    let dir = setup_cargo_project("pub fn ok() -> i32 { 42 }\n");
    let report: BuildReport = run_check(dir.path(), "rust", &default_settings())
        .expect("check should run for rust + Cargo.toml");
    assert!(
        report.passed,
        "expected pass, got errors: {:?}",
        report.errors
    );
    assert!(report.errors.is_empty());
    assert_eq!(report.checker, "cargo check");
}

#[test]
fn rust_check_fails_on_syntax_error() {
    let dir = setup_cargo_project("pub fn broken() -> i32 { \n");
    let report = run_check(dir.path(), "rust", &default_settings()).expect("check should run");
    assert!(!report.passed);
    assert!(
        !report.errors.is_empty(),
        "expected at least one error on syntax-broken input"
    );
}

#[test]
fn rust_check_fails_on_name_resolution_error() {
    // The `&bn` case from #113: syntactically valid (`bn` is a valid
    // ident), semantically broken (unresolved name). Syntax Guard's
    // blind spot, this check's whole reason for being.
    let dir = setup_cargo_project("pub fn x() -> Option<&'static u8> { Some(&bn) }\n");
    let report = run_check(dir.path(), "rust", &default_settings()).expect("check should run");
    assert!(
        !report.passed,
        "name-resolution error should fail the check"
    );
    let joined = report
        .errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("bn") || joined.contains("cannot find") || joined.contains("not found"),
        "expected error to mention the missing ident, got: {joined}"
    );
}

#[test]
fn rust_check_returns_none_without_cargo_toml() {
    let dir = tempfile::tempdir().unwrap();
    let report = run_check(dir.path(), "rust", &default_settings());
    assert!(
        report.is_none(),
        "no Cargo.toml → no check; got: {report:?}"
    );
}

#[test]
fn rust_check_returns_none_when_disabled_in_config() {
    let dir = setup_cargo_project("pub fn ok() -> i32 { 42 }\n");
    let disabled = EditSettings {
        native_check: false,
        native_check_timeout_secs: 10,
    };
    let report = run_check(dir.path(), "rust", &disabled);
    assert!(report.is_none(), "disabled config → no check");
}

#[test]
fn check_returns_none_for_unsupported_lang() {
    // Java, C#, etc. are out of scope for this slice.
    let dir = setup_cargo_project("pub fn ok() -> i32 { 42 }\n");
    let report = run_check(dir.path(), "java", &default_settings());
    assert!(report.is_none());
}

// ─── wait_with_timeout stderr-capture contract (Copilot #5) ───────────
//
// The `drain_once` polling implementation races against child exit:
// `ChildStderr::read()` is blocking, so the main loop only reads when
// it happens to poll between writes. CI intermittently saw empty
// stderr for `cargo check` runs that finished before the first drain
// pass. These tests pin the contract directly against
// `wait_with_timeout` via a small synthetic child, independent of
// `cargo`'s timing.

/// Fast-exit child that writes stderr and exits immediately: the
/// worst-case race for a polling drainer. A reader-thread
/// implementation captures the full stderr regardless of timing.
#[test]
fn wait_with_timeout_captures_stderr_from_fast_exiting_child() {
    use super::{wait_with_timeout, WaitOutcome};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    // We repeat because the race is inherently timing-dependent —
    // even 1 miss in 20 iterations is a real flake.
    for iter in 0..20 {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("printf 'FAST_STDERR_MARKER\\n' 1>&2; exit 7")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn sh");

        let outcome = wait_with_timeout(&mut child, Duration::from_secs(5));
        match outcome {
            WaitOutcome::Exited { status, stderr } => {
                assert_eq!(status.code(), Some(7), "iter {iter}: exit code");
                assert!(
                    stderr.contains("FAST_STDERR_MARKER"),
                    "iter {iter}: stderr lost — race captured. got: {stderr:?}"
                );
            }
            other => panic!("iter {iter}: unexpected outcome {other:?}"),
        }
    }
}

/// Delayed-stderr child: stderr is written only after a short pause,
/// mimicking a real compiler that warms up before emitting diagnostics.
/// A blocking-drain impl hangs here until the write arrives; a
/// reader-thread impl doesn't care either way.
#[test]
fn wait_with_timeout_captures_stderr_from_delayed_child() {
    use super::{wait_with_timeout, WaitOutcome};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let mut child = Command::new("sh")
        .arg("-c")
        .arg("sleep 0.15; printf 'LATE_STDERR_MARKER\\n' 1>&2; exit 3")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sh");

    let outcome = wait_with_timeout(&mut child, Duration::from_secs(5));
    match outcome {
        WaitOutcome::Exited { status, stderr } => {
            assert_eq!(status.code(), Some(3));
            assert!(
                stderr.contains("LATE_STDERR_MARKER"),
                "stderr lost on delayed write: got {stderr:?}"
            );
        }
        other => panic!("unexpected outcome {other:?}"),
    }
}

/// Large-stderr child: write well beyond a typical pipe-buffer chunk
/// (4 KiB) so a single-chunk drainer would lose tail bytes. All lines
/// must survive — we check the first and last marker.
#[test]
fn wait_with_timeout_captures_large_stderr() {
    use super::{wait_with_timeout, WaitOutcome};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    // ~800 * 40 bytes ≈ 32 KiB, far beyond STDERR_CHUNK_BYTES (4 KiB).
    let script = r#"
        i=0
        while [ $i -lt 800 ]; do
            printf 'line_%04d_marker_xxxxxxxxxxxxxxxxxx\n' $i 1>&2
            i=$((i+1))
        done
        exit 1
    "#;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sh");

    let outcome = wait_with_timeout(&mut child, Duration::from_secs(10));
    match outcome {
        WaitOutcome::Exited { status, stderr } => {
            assert_eq!(status.code(), Some(1));
            assert!(
                stderr.contains("line_0000_marker"),
                "first line missing: first 200 chars: {:?}",
                stderr.chars().take(200).collect::<String>()
            );
            assert!(
                stderr.contains("line_0799_marker"),
                "last line missing — drain truncated large stderr: last 200 chars: {:?}",
                stderr
                    .chars()
                    .rev()
                    .take(200)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
        }
        other => panic!("unexpected outcome {other:?}"),
    }
}
