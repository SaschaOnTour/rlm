//! Shared helpers for CLI command handlers.
//!
//! Post-0.5.0 the CLI adapter only does three things that need shared
//! plumbing: translating application errors into the CLI's
//! `CmdResult` box, printing pre-serialised bodies, and resolving the
//! `--code` / `--code-stdin` / `--code-file` family for write
//! commands. Everything else — config/DB open, savings recording,
//! query pipelines — moved into [`RlmSession`](crate::application::session::RlmSession).

use crate::output::{self, Formatter};

pub type CmdResult = Result<(), Box<dyn std::fmt::Display>>;

pub fn print_str(formatter: Formatter, s: &str) {
    output::print_str(formatter, s);
}

pub fn map_err(e: impl std::fmt::Display + 'static) -> Box<dyn std::fmt::Display> {
    Box::new(e.to_string())
}

/// Resolve the code body for `rlm replace` / `rlm insert` from its three
/// possible sources: `--code <inline>`, `--code-stdin`, or
/// `--code-file <path>`. Clap enforces mutual exclusivity via the `group`
/// attribute; this helper enforces "exactly one" by rejecting the
/// none-specified case and by reading the chosen source.
///
/// Error cases:
/// * None of the three specified → "no code source provided".
/// * `--code-stdin` on an interactive TTY → refuse (agents should pipe).
/// * `--code-file` on a missing or non-file path → "not a readable file".
/// * `--code-stdin` with non-UTF-8 bytes → bubbled from `read_to_string`.
pub fn resolve_code(
    code: Option<&str>,
    code_stdin: bool,
    code_file: Option<&str>,
) -> Result<String, Box<dyn std::fmt::Display>> {
    match (code, code_stdin, code_file) {
        (Some(s), false, None) => Ok(s.to_string()),
        (None, true, None) => read_stdin_code(),
        (None, false, Some(path)) => read_file_code(path),
        (None, false, None) => Err(map_err(
            "one of --code, --code-stdin, or --code-file is required",
        )),
        _ => Err(map_err(
            "--code, --code-stdin, and --code-file are mutually exclusive",
        )),
    }
}

fn read_stdin_code() -> Result<String, Box<dyn std::fmt::Display>> {
    use std::io::{IsTerminal, Read};
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(map_err(
            "--code-stdin reads from stdin but stdin is a TTY; pipe or redirect the code",
        ));
    }
    let mut buf = String::new();
    stdin
        .lock()
        .read_to_string(&mut buf)
        .map_err(|e| map_err(format!("failed to read stdin: {e}")))?;
    Ok(buf)
}

fn read_file_code(path: &str) -> Result<String, Box<dyn std::fmt::Display>> {
    let p = std::path::Path::new(path);
    if !p.is_file() {
        return Err(map_err(format!(
            "--code-file path is not a readable file: {path}"
        )));
    }
    std::fs::read_to_string(p).map_err(|e| map_err(format!("failed to read {path}: {e}")))
}
