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

const CONFIG_DIR: &str = ".rlm";
const CONFIG_FILE: &str = "config.toml";
const DEFAULT_FORMAT: &str = "toon";

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
        State::FileWithoutOutput(existing) => write_with_appended_output(&config_path, &existing)?,
        State::FormatAlreadySet => {}
    }
    Ok(action)
}

enum State {
    NoFile,
    FileWithoutOutput(String),
    FormatAlreadySet,
}

fn inspect(config_path: &Path) -> Result<State> {
    if !config_path.exists() {
        return Ok(State::NoFile);
    }
    let content = fs::read_to_string(config_path)?;
    if has_output_format(&content) {
        Ok(State::FormatAlreadySet)
    } else {
        Ok(State::FileWithoutOutput(content))
    }
}

fn classify_action(state: &State, mode: SetupMode) -> SetupAction {
    let check = matches!(mode, SetupMode::Check);
    match state {
        State::NoFile if check => SetupAction::WouldCreate,
        State::NoFile => SetupAction::Created,
        State::FileWithoutOutput(_) if check => SetupAction::WouldUpdate,
        State::FileWithoutOutput(_) => SetupAction::Updated,
        State::FormatAlreadySet => SetupAction::Skipped,
    }
}

/// Detect whether `[output]` already has `format = "..."` set.
/// Simple line-based scan avoids depending on a TOML parser for this
/// single check.
///
/// Key matching is **exact**: only a `format` key counts, not
/// `formatting` / `formatter` / `format_version` / etc. The early
/// prefix-match (`starts_with("format")`) incorrectly swallowed all of
/// those and silently suppressed the real `format` line write
/// (Copilot finding).
fn has_output_format(content: &str) -> bool {
    let mut in_output = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_output = line.eq_ignore_ascii_case("[output]");
            continue;
        }
        if in_output && is_format_key_line(line) {
            return true;
        }
    }
    false
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
    fs::write(path, body)?;
    Ok(())
}

fn write_with_appended_output(path: &Path, existing: &str) -> Result<()> {
    let separator = if existing.ends_with('\n') { "" } else { "\n" };
    let appended = format!(
        "{existing}{separator}\n\
         # Added by `rlm setup` — TOON for token density on flat responses.\n\
         [output]\n\
         format = \"{DEFAULT_FORMAT}\"\n"
    );
    fs::write(path, appended)?;
    Ok(())
}

#[cfg(test)]
#[path = "config_format_tests.rs"]
mod tests;
