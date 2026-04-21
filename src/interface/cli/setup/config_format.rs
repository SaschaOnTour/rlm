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
/// non-`FormatAlreadySet` variants each take a distinct write path,
/// because appending a fresh `[output]` table when one already
/// exists would produce a **duplicate** section and invalid TOML
/// (Copilot finding).
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
    if !config_path.exists() {
        return Ok(State::NoFile);
    }
    let content = fs::read_to_string(config_path)?;
    Ok(match classify_output(&content) {
        OutputLocation::AbsentSection => State::NoOutputSection(content),
        OutputLocation::SectionWithoutFormat => State::OutputWithoutFormat(content),
        OutputLocation::SectionWithFormat => State::FormatAlreadySet,
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

/// Where is `[output].format` relative to the rest of the config?
enum OutputLocation {
    /// No `[output]` section anywhere.
    AbsentSection,
    /// `[output]` section exists, but no `format` key inside it.
    SectionWithoutFormat,
    /// `[output]` section exists and contains a `format` key.
    SectionWithFormat,
}

/// Simple line-based scan — avoids depending on a TOML parser for this
/// single check. Key matching is **exact**: only a `format` key
/// counts, not `formatting` / `formatter` / `format_version` / etc.
/// (The old prefix-match silently suppressed the real `format` line
/// write when such lookalikes were present.)
fn classify_output(content: &str) -> OutputLocation {
    let mut in_output = false;
    let mut saw_output = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_output = line.eq_ignore_ascii_case("[output]");
            if in_output {
                saw_output = true;
            }
            continue;
        }
        if in_output && is_format_key_line(line) {
            return OutputLocation::SectionWithFormat;
        }
    }
    if saw_output {
        OutputLocation::SectionWithoutFormat
    } else {
        OutputLocation::AbsentSection
    }
}

/// A TOML key/value line whose key is exactly `format` (ignoring
/// whitespace on either side of the `=`). Trailing value is not
/// validated — we only care about detecting the key's presence.
fn is_format_key_line(line: &str) -> bool {
    let Some((key, _value)) = line.split_once('=') else {
        return false;
    };
    key.trim() == "format"
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
/// the end, separated from prior content by a blank line.
fn write_with_appended_section(path: &Path, existing: &str) -> Result<()> {
    let separator = if existing.ends_with('\n') { "" } else { "\n" };
    let appended = format!(
        "{existing}{separator}\n\
         # Added by `rlm setup` — TOON for token density on flat responses.\n\
         [output]\n\
         format = \"{DEFAULT_FORMAT}\"\n"
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

    for line in existing.split_inclusive('\n') {
        out.push_str(line);
        // `split_inclusive` keeps the trailing `\n` on the line, so a
        // bare header line comes through as `"[output]\n"`. Trim
        // before comparing, then emit the injected key right after.
        if !injected && line.trim().eq_ignore_ascii_case("[output]") {
            out.push_str(&format!("format = \"{DEFAULT_FORMAT}\"\n"));
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

#[cfg(test)]
#[path = "config_format_tests.rs"]
mod tests;
