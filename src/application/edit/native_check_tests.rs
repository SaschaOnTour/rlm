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
    assert!(!report.passed, "report: {report:#?}");
    assert!(
        !report.errors.is_empty(),
        "expected at least one error on syntax-broken input. \
         Full BuildReport: {report:#?}"
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
        "name-resolution error should fail the check. \
         Full BuildReport: {report:#?}"
    );
    let joined = report
        .errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("bn") || joined.contains("cannot find") || joined.contains("not found"),
        "expected error to mention the missing ident. \
         Full BuildReport: {report:#?}, \
         joined error messages: {joined:?}"
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

// ─── Cargo cache isolation (CI regression) ────────────────────────────
//
// When the parent process has `CARGO_TARGET_DIR` set (e.g. under
// `cargo nextest run` on CI), that env var leaks into the `cargo
// check` subprocess we spawn. Every test's probe project has the
// same package name / version, so cargo's fingerprint cache in the
// shared target dir returns "already built" and exits immediately
// without running rustc — leaving `BuildReport { passed: true,
// errors: [] }` regardless of whether the source is broken.
//
// The fix (in `spawn_cargo_check`) removes `CARGO_TARGET_DIR` from
// the subprocess env so cargo falls back to the project's own
// `./target`. This test pins that fix by simulating the leaked env
// var explicitly. Note the serial_test attribute would be the clean
// way to guard against env-var data races; rlm doesn't depend on it,
// so we restore the env on scope exit with a Drop guard instead.

/// Scope guard that restores (or removes) an env var on drop so one
/// test's env mutation doesn't leak to siblings.
struct EnvVarGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// Capture raw cargo-check output with the same env-hygiene rules
/// `spawn_cargo_check` applies, so the regression test can print what
/// cargo actually says on the failing platform.
fn raw_cargo_check(project: &std::path::Path) -> String {
    use std::process::Command;
    let output = Command::new("cargo")
        .arg("check")
        .arg("--message-format")
        .arg("short")
        .arg("--quiet")
        .current_dir(project)
        .env("CARGO_TARGET_DIR", project.join("target"))
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("CARGO_BUILD_RUSTC_WRAPPER")
        .output()
        .expect("spawn cargo for diagnostics");
    format!(
        "exit_status: {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn cargo_check_ignores_inherited_cargo_target_dir() {
    // Simulate a leaked CARGO_TARGET_DIR — the exact shape the CI
    // runner gets when nextest sets it for the parent test process.
    let shared = tempfile::tempdir().expect("shared target tempdir");
    let _guard = EnvVarGuard::set("CARGO_TARGET_DIR", shared.path());

    // First: a broken-syntax project. Must report errors — not the
    // cache-hit "passed: true, errors: []" we saw on CI.
    let broken = setup_cargo_project("pub fn broken() -> i32 { \n");
    let report = run_check(broken.path(), "rust", &default_settings()).expect("check should run");

    // Diagnostic: if the assertion below fails, show what cargo
    // actually emitted on this platform. Computed lazily via
    // `raw_cargo_check` — only paid when the assertion message is
    // formatted (on panic).
    let diag = || raw_cargo_check(broken.path());
    assert!(
        !report.passed,
        "broken source must fail the check. report: {report:#?}; raw cargo: {}",
        diag()
    );
    assert!(
        !report.errors.is_empty(),
        "broken source must have at least one error. report: {report:#?}; raw cargo: {}",
        diag()
    );

    // Second: a separate valid project that would share the shared
    // target dir's fingerprint cache if we inherited the env. It must
    // still do its own full build (project-local ./target).
    let valid = setup_cargo_project("pub fn ok() -> i32 { 42 }\n");
    let report = run_check(valid.path(), "rust", &default_settings()).expect("check should run");
    let diag_valid = || raw_cargo_check(valid.path());
    assert!(
        report.passed,
        "valid source must pass. report: {report:#?}; raw cargo: {}",
        diag_valid()
    );
    assert!(
        valid.path().join("target").exists(),
        "cargo check must build into the project's own ./target. raw cargo: {}",
        diag_valid()
    );
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
