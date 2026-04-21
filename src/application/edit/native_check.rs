//! Post-write native checker (task #115).
//!
//! Tree-sitter's Syntax Guard validates that a write produces a
//! *parseable* file. It cannot check name resolution, lifetimes, type
//! bounds, module paths, or anything else the language's real
//! front-end verifies. For Rust, running `cargo check` right after
//! every `rlm replace/insert/delete` closes that gap; the result goes
//! into the write-response JSON as a `build: { passed, errors, … }`
//! field so agents see the failure without a second tool call.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::config::EditSettings;

/// Polling interval for `Child::try_wait`. 50 ms is well below human
/// perceptibility while keeping CPU use trivial for a seconds-scale
/// budget.
const WAIT_POLL_MS: u64 = 50;

/// Max pieces `parse_location` extracts from a diagnostic line
/// (`path:line:col:rest`).
const LOCATION_SPLIT: usize = 4;

/// Outcome of a native check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BuildReport {
    pub checker: String,
    pub passed: bool,
    pub errors: Vec<BuildError>,
    pub duration_ms: u64,
}

/// One diagnostic line from the checker, parsed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BuildError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub message: String,
}

/// Decide whether a check applies and run it.
///
/// Returns `None` when the config disables checking, the language has
/// no supported checker, or the project marker is missing. Subprocess
/// failures fold into the returned `BuildReport` instead of raising.
pub fn run_check(project_root: &Path, lang: &str, settings: &EditSettings) -> Option<BuildReport> {
    if !settings.native_check {
        return None;
    }
    dispatch_check(project_root, lang, settings)
}

fn dispatch_check(project_root: &Path, lang: &str, settings: &EditSettings) -> Option<BuildReport> {
    match lang {
        "rust" => run_cargo_check(project_root, settings),
        _ => None,
    }
}

// ─── Rust / cargo check ─────────────────────────────────────────────────

fn run_cargo_check(project_root: &Path, settings: &EditSettings) -> Option<BuildReport> {
    if !project_root.join("Cargo.toml").exists() {
        return None;
    }
    let timeout = Duration::from_secs(settings.native_check_timeout_secs);
    let started = Instant::now();
    Some(execute_cargo_check(project_root, timeout, started))
}

fn execute_cargo_check(project_root: &Path, timeout: Duration, started: Instant) -> BuildReport {
    let mut child = match spawn_cargo_check(project_root) {
        Ok(c) => c,
        Err(e) => {
            return error_only_report(
                "cargo check",
                started,
                format!("failed to spawn cargo: {e}"),
            );
        }
    };
    match wait_with_timeout(&mut child, timeout) {
        WaitOutcome::Exited { status, stderr } => finish_exited(status, stderr, started),
        WaitOutcome::TimedOut => {
            kill_and_reap(&mut child);
            error_only_report(
                "cargo check",
                started,
                format!(
                    "cargo check timed out after {}s — partial diagnostics suppressed",
                    timeout.as_secs()
                ),
            )
        }
        WaitOutcome::Io(e) => {
            error_only_report("cargo check", started, format!("cargo check failed: {e}"))
        }
    }
}

fn spawn_cargo_check(project_root: &Path) -> std::io::Result<Child> {
    // Three env-hygiene rules for the subprocess cargo. Each was
    // motivated by a real CI-only failure; keep them together.
    //
    // 1. **Pin CARGO_TARGET_DIR to `<project>/target`.** rlm runs as
    //    a child of some caller (agent, IDE, `cargo nextest run`)
    //    that may have its own `CARGO_TARGET_DIR` set for reasons
    //    unrelated to ours. Cargo's fingerprint cache keys on
    //    `(package name, version, source)`, not path — two
    //    identically-named probe projects land on the same cache
    //    entry and the second one "succeeds" instantly without
    //    rustc. Explicitly pointing at the project-local `target/`
    //    overrides any inherited value (plain `env_remove` isn't
    //    enough on CI — some runners re-inject it through other
    //    Cargo env vars).
    //
    // 2. **Disable terminal colors.** Our stderr parser keys on
    //    lines starting with `error` / containing `: error`. ANSI
    //    color escapes (`\x1b[1;31merror…`) break both checks and
    //    silently yield empty `errors`.
    //
    // 3. **Bypass `RUSTC_WRAPPER`.** Wrappers like `sccache` cache
    //    `rustc` output by input hash; a misconfigured wrapper can
    //    short-circuit the real compile. For a correctness check
    //    rlm cares about accuracy, not wrapper-speed.
    Command::new("cargo")
        .arg("check")
        .arg("--message-format")
        .arg("short")
        .arg("--quiet")
        .current_dir(project_root)
        .env("CARGO_TARGET_DIR", project_root.join("target"))
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("CARGO_BUILD_RUSTC_WRAPPER")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

fn finish_exited(
    status: std::process::ExitStatus,
    stderr: String,
    started: Instant,
) -> BuildReport {
    let errors = parse_cargo_short_stderr(&stderr);
    BuildReport {
        checker: "cargo check".to_string(),
        passed: status.success() && errors.is_empty(),
        errors,
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

// ─── Subprocess wait with timeout ───────────────────────────────────────

#[cfg_attr(test, derive(Debug))]
pub(crate) enum WaitOutcome {
    Exited {
        status: std::process::ExitStatus,
        stderr: String,
    },
    TimedOut,
    Io(std::io::Error),
}

/// Wait for the child with a wall-clock timeout.
///
/// A dedicated reader thread consumes the full `stderr` stream so the
/// main loop only has to `try_wait()` + check the deadline. The
/// previous polling-drain approach called `ChildStderr::read()` —
/// which is blocking — from the same loop as the timeout check, and
/// races against a fast-exiting child: if the child exits before the
/// main loop's first drain pass, stderr can be silently lost or
/// truncated depending on OS pipe buffering (Copilot finding #5).
/// The reader thread reads until EOF, which happens when the pipe
/// closes (child exit or explicit `kill`), so capture is
/// timing-independent.
pub(crate) fn wait_with_timeout(child: &mut Child, timeout: Duration) -> WaitOutcome {
    let stderr_reader = spawn_stderr_reader(child.stderr.take());
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = collect_stderr(stderr_reader);
                return WaitOutcome::Exited { status, stderr };
            }
            Ok(None) if Instant::now() >= deadline => return WaitOutcome::TimedOut,
            Ok(None) => std::thread::sleep(Duration::from_millis(WAIT_POLL_MS)),
            Err(e) => return WaitOutcome::Io(e),
        }
    }
}

/// Drain a `ChildStderr` to completion on a worker thread. Returns
/// `None` when the child was spawned without a captured stderr.
fn spawn_stderr_reader(pipe: Option<std::process::ChildStderr>) -> Option<JoinHandle<String>> {
    pipe.map(|mut p| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            // `read_to_string` blocks until EOF — i.e. the child exits
            // or is killed. A non-UTF-8 byte sequence is rare for
            // `cargo` output but we silently keep whatever UTF-8 was
            // read up to that point.
            let _ = p.read_to_string(&mut buf);
            buf
        })
    })
}

/// Join the reader thread and return whatever it collected. A panic
/// inside the reader (shouldn't happen — `read_to_string` can't
/// panic on its own) or a thread-join failure yields an empty string
/// rather than propagating.
fn collect_stderr(reader: Option<JoinHandle<String>>) -> String {
    reader.and_then(|h| h.join().ok()).unwrap_or_default()
}

// ─── Diagnostic parsing ────────────────────────────────────────────────

fn parse_cargo_short_stderr(stderr: &str) -> Vec<BuildError> {
    stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| l.starts_with("error") || l.contains(": error"))
        .map(build_error_from_line)
        .collect()
}

fn build_error_from_line(line: &str) -> BuildError {
    let (file, line_no) = parse_location(line);
    BuildError {
        file,
        line: line_no,
        message: line.to_string(),
    }
}

fn parse_location(line: &str) -> (Option<String>, Option<u32>) {
    let mut it = line.splitn(LOCATION_SPLIT, ':');
    let (first, second, third) = (it.next(), it.next(), it.next());
    match (first, second, third) {
        (Some(path), Some(line_s), Some(col_s)) if is_numeric(line_s) && is_numeric(col_s) => {
            (Some(path.to_string()), line_s.parse().ok())
        }
        _ => (None, None),
    }
}

fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn error_only_report(checker: &str, started: Instant, msg: String) -> BuildReport {
    BuildReport {
        checker: checker.to_string(),
        passed: false,
        errors: vec![BuildError {
            file: None,
            line: None,
            message: msg,
        }],
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

#[cfg(test)]
#[path = "native_check_tests.rs"]
mod tests;
