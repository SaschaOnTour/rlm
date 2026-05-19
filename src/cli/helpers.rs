//! Shared helpers for CLI command handlers.
//!
//! Post-0.5.0 the CLI adapter only does three things that need shared
//! plumbing: translating application errors into the CLI's
//! `CmdResult` box, handing the cwd to facades as the project root,
//! and resolving the `--code` / `--code-stdin` / `--code-file` family
//! for write commands. Everything else — config/DB open, savings
//! recording, query pipelines — moved into
//! [`RlmSession`](crate::application::session::RlmSession).
//!
//! Note: `cwd_project_root` does **not** walk up the directory tree
//! to find a `.rlm/` or `.git/` anchor — it just returns
//! `std::env::current_dir()`. Project-root discovery is a feature
//! we don't have yet; the assumption is "run rlm from the project
//! root". If that ever becomes a sharp edge in practice, this is
//! the function to extend.

use crate::cli::commands::CodeSource;

pub type CmdResult = Result<(), Box<dyn std::fmt::Display>>;

pub(crate) fn map_err(e: impl std::fmt::Display + 'static) -> Box<dyn std::fmt::Display> {
    Box::new(e.to_string())
}

/// CLI-side wrapper around `std::env::current_dir()` so adapters can
/// pass the cwd into an `application::facades::*_project` call (the
/// single application entry point per command) without going through
/// a `Config` round-trip — `Config::new` then runs once inside
/// `RlmSession::open` instead of twice. MCP doesn't use this; the
/// MCP server is constructed with an explicit project root.
pub(crate) fn cwd_project_root() -> Result<std::path::PathBuf, Box<dyn std::fmt::Display>> {
    std::env::current_dir().map_err(map_err)
}

/// Run a facade call against the cwd-discovered project root, then
/// print its already-serialised JSON body via the active formatter.
/// Collapses the `let root = cwd_project_root()?; let body =
/// facades::X(&root, ..).map_err(map_err)?; print_str(formatter,
/// &body); Ok(())` pattern repeated across every read-side cmd
/// handler.
///
/// For facades returning `OperationResponse`, callers pre-extract
/// the body with `.map(|r| r.body)`; for facades returning a raw
/// JSON `String`, they pass the result through directly.
pub(crate) fn run_facade(
    formatter: crate::output::Formatter,
    f: impl FnOnce(&std::path::Path) -> crate::error::Result<String>,
) -> CmdResult {
    let root = cwd_project_root()?;
    let body = f(&root).map_err(map_err)?;
    crate::output::print_str(formatter, &body);
    Ok(())
}

/// Resolve the code body for `rlm replace` / `rlm insert` from its
/// `CodeSource` bundle. Clap's `group(required = true, multiple = false)`
/// already enforces "exactly one of `--code` / `--code-stdin` /
/// `--code-file`" at parse time, so this helper only handles the
/// I/O — read stdin or read the file path.
///
/// Error cases:
/// * `--code-stdin` on an interactive TTY → refuse (agents should pipe).
/// * `--code-file` on a missing or non-file path → "not a readable file".
/// * `--code-stdin` with non-UTF-8 bytes → bubbled from `read_to_string`.
pub(crate) fn resolve_code(src: &CodeSource) -> Result<String, Box<dyn std::fmt::Display>> {
    match (
        src.code.as_deref(),
        src.code_stdin,
        src.code_file.as_deref(),
    ) {
        (Some(s), false, None) => Ok(s.to_string()),
        (None, true, None) => read_stdin_code(),
        (None, false, Some(path)) => read_file_code(path),
        // Other shapes are unreachable: clap rejects them at parse time
        // via the `code_src` group constraints.
        _ => Err(map_err(
            "internal: clap's CodeSource group should make this unreachable",
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
