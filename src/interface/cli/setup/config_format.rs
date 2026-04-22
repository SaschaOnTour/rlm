//! `rlm setup` step: ensure `.rlm/config.toml` carries `[output] format`
//! set to `"toon"` (task #121).
//!
//! TOON is 30-50% token-denser than JSON for flat tabular responses
//! (search, refs, files, stats). Setting it by default when the
//! project was scoped for agent use (`rlm setup` ran) gets the
//! savings automatically without every agent having to rediscover the
//! preference.
//!
//! Idempotent and respectful: existing `format` preferences are
//! preserved untouched. `Remove` mode leaves this alone entirely —
//! format is user data, not an rlm marker.
//!
//! This module writes the TOML by hand (append a section / rewrite
//! the value line) rather than deserialising + re-serialising the
//! whole file. That preserves the user's comments and formatting.
//! The parser is deliberately simple — only recognises the
//! `[output]` section header and `format = "..."` line, everything
//! else flows through byte-for-byte.

use std::fs;
use std::path::Path;

use super::orchestrator::{SetupAction, SetupMode};
use crate::error::Result;
use crate::infrastructure::filesystem::atomic_writer::write_atomic;

const CONFIG_DIR: &str = ".rlm";
const CONFIG_FILE: &str = "config.toml";
const DEFAULT_FORMAT: &str = "toon";

/// Pre-allocation hint for `write_with_injected_format`'s output
/// buffer — roughly the length of `format = "toon"\n` plus slack for
/// editor-style variations (tabs vs spaces, trailing newline).
const INJECTED_FORMAT_LINE_CAPACITY: usize = 32;

/// Ensure `.rlm/config.toml` has `[output] format = "toon"` unless the
/// user already set a preference. Reports what happened for the
/// setup report.
pub fn setup_config_format(project_dir: &Path, mode: SetupMode) -> Result<SetupAction> {
    let config_path = project_dir.join(CONFIG_DIR).join(CONFIG_FILE);

    // `Remove` leaves format alone (it's user preference, not a marker).
    if matches!(mode, SetupMode::Remove) {
        return Ok(SetupAction::Skipped);
    }

    let current_state = inspect(&config_path)?;
    let action = classify_action(&current_state, mode);
    if matches!(mode, SetupMode::Check) {
        return Ok(action);
    }

    match current_state {
        State::NoFile => write_fresh_config(&config_path)?,
        State::NoOutputSection(existing) => write_with_appended_section(&config_path, &existing)?,
        State::OutputWithoutFormat(existing) => {
            write_with_injected_format(&config_path, &existing)?
        }
        State::FormatAlreadySet => {}
    }
    Ok(action)
}

/// Classification of an existing (or absent) `config.toml` relative to
/// the question "is the `[output].format` key set?". The three
/// non-`FormatAlreadySet` variants each take a distinct write path:
/// appending a fresh `[output]` table when one already exists would
/// produce a duplicate section and invalid TOML.
enum State {
    /// No `config.toml` on disk at all.
    NoFile,
    /// File exists, has no `[output]` section anywhere. Safe to
    /// append a fresh `[output]` table.
    NoOutputSection(String),
    /// File exists, has an `[output]` section, but the section does
    /// not contain a `format = …` key. Must inject the key **inside
    /// the existing section** rather than append a second one.
    OutputWithoutFormat(String),
    /// File exists and `[output].format` is already set — user
    /// preference takes precedence.
    FormatAlreadySet,
}

fn inspect(config_path: &Path) -> Result<State> {
    // Direct read + ErrorKind::NotFound match rather than
    // `Path::exists()`: the latter returns `false` on permission/I/O
    // errors too, which would funnel an unreadable file into the
    // "create fresh" path and clobber it.
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::NoFile),
        Err(e) => return Err(e.into()),
    };

    // Classify via the toml crate itself instead of a hand-rolled
    // line scan — the parser handles trailing comments, alternative
    // quoting, nested tables, whitespace, and all the edge cases
    // we'd otherwise have to reimplement. Malformed TOML folds into
    // `NoOutputSection`: the writer then appends a fresh `[output]`
    // table, which is the best we can do without second-guessing
    // the user's mistake.
    let output_table = toml::from_str::<toml::Value>(&content)
        .ok()
        .and_then(|v| v.get("output").and_then(|o| o.as_table().cloned()));

    Ok(match output_table {
        Some(tbl) if tbl.contains_key("format") => State::FormatAlreadySet,
        Some(_) => State::OutputWithoutFormat(content),
        None => State::NoOutputSection(content),
    })
}

fn classify_action(state: &State, mode: SetupMode) -> SetupAction {
    let check = matches!(mode, SetupMode::Check);
    match state {
        State::NoFile if check => SetupAction::WouldCreate,
        State::NoFile => SetupAction::Created,
        State::NoOutputSection(_) | State::OutputWithoutFormat(_) if check => {
            SetupAction::WouldUpdate
        }
        State::NoOutputSection(_) | State::OutputWithoutFormat(_) => SetupAction::Updated,
        State::FormatAlreadySet => SetupAction::Skipped,
    }
}

fn write_fresh_config(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = format!(
        "# Auto-written by `rlm setup` for Claude Code projects.\n\
         # TOON is ~30-50% token-denser than JSON on flat responses\n\
         # (search, refs, files, stats). Override with format = \"json\"\n\
         # or format = \"pretty\" if you prefer human-readable output.\n\
         [output]\n\
         format = \"{DEFAULT_FORMAT}\"\n"
    );
    write_atomic(path, body.as_bytes())?;
    Ok(())
}

/// Existing file has no `[output]` section. Append a fresh one at
/// the end, separated from prior content by a blank line. Uses the
/// file's own line-ending style so a CRLF config stays pure CRLF.
fn write_with_appended_section(path: &Path, existing: &str) -> Result<()> {
    let eol = detect_eol(existing);
    let separator = if existing.ends_with('\n') { "" } else { eol };
    let appended = format!(
        "{existing}{separator}{eol}\
         # Added by `rlm setup` — TOON for token density on flat responses.{eol}\
         [output]{eol}\
         format = \"{DEFAULT_FORMAT}\"{eol}"
    );
    write_atomic(path, appended.as_bytes())?;
    Ok(())
}

/// Existing file already has an `[output]` section (with other
/// keys). Inject `format = "..."` as the first key inside that
/// section so we don't emit a second `[output]` table. User's other
/// keys and comments stay byte-for-byte untouched.
fn write_with_injected_format(path: &Path, existing: &str) -> Result<()> {
    let mut out = String::with_capacity(existing.len() + INJECTED_FORMAT_LINE_CAPACITY);
    let mut injected = false;
    let trailing_nl = existing.ends_with('\n');
    // Match the file's existing line-ending style so injected lines
    // don't introduce mixed EOLs (e.g. `\n` inside an otherwise CRLF
    // file on Windows). Files with no newline at all fall back to LF.
    let eol = detect_eol(existing);

    for line in existing.split_inclusive('\n') {
        out.push_str(line);
        // The header detector is the same one `classify_output` uses,
        // so a line like `"[output]   # note\n"` matches just like a
        // bare `"[output]\n"`. Emit the injected key on the line
        // after the header, with the file's own EOL.
        if !injected && is_output_header(line) {
            out.push_str(&format!("format = \"{DEFAULT_FORMAT}\"{eol}"));
            injected = true;
        }
    }

    // `split_inclusive` preserves the original trailing-newline
    // state; ensure we did not accidentally add one when `existing`
    // lacked one.
    if !trailing_nl && out.ends_with('\n') {
        out.pop();
    }

    write_atomic(path, out.as_bytes())?;
    Ok(())
}

/// Detect the dominant line ending in `content`: CRLF if any line is
/// CRLF-terminated, otherwise LF. Good enough for config files that
/// conventionally use one style throughout; mixed-EOL files keep
/// whatever we first see.
fn detect_eol(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// True if `raw` (possibly with trailing `\n`) is an `[output]` table
/// header — tolerates trailing whitespace and `# comment`, rejects
/// array-of-tables (`[[output]]`). Only needed by the write path;
/// the read path uses `toml::from_str` directly.
fn is_output_header(raw: &str) -> bool {
    let line = raw.trim();
    let before_comment = line.split_once('#').map_or(line, |(pre, _)| pre).trim_end();
    let Some(inner) = before_comment
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
    else {
        return false;
    };
    // Reject `[[...]]` — we only handle plain tables.
    if inner.starts_with('[') || inner.ends_with(']') {
        return false;
    }
    inner.trim().eq_ignore_ascii_case("output")
}

#[cfg(test)]
#[path = "config_format_tests.rs"]
mod tests;
