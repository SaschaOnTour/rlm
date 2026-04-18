//! Top-level orchestration for `rlm setup`.
//!
//! Owns the shared public types (`SetupMode`, `SetupAction`, `SetupReport`,
//! `SetupError`), the `run_setup` entrypoint that dispatches to the
//! per-concern sub-modules, the `setup_initial_index` step, and the
//! `write_atomic` + `replace_file` primitives consumed by `settings` and
//! `claude_md`. Slice 5.2 will pull the atomic-write primitives into
//! `infrastructure/filesystem/atomic_writer` so every write path (setup,
//! edit/validator) shares one implementation.

use std::path::Path;

use serde::Serialize;
use thiserror::Error;

use crate::config::Config;
use crate::error::Result;

use super::{claude_md, settings};

/// Failures specific to `rlm setup`.
#[derive(Error, Debug)]
pub enum SetupError {
    /// The existing settings file is valid JSON but not an object; we refuse
    /// to overwrite user content of unknown shape.
    #[error("{path} is not a JSON object — rlm refuses to overwrite it. Remove or replace the file before re-running setup.")]
    NotJsonObject { path: String },

    /// The existing settings file is not parseable JSON.
    #[error("{path} is not valid JSON ({source}) — rlm refuses to overwrite it. Fix the file before re-running setup.")]
    InvalidJson {
        path: String,
        source: serde_json::Error,
    },

    /// The atomic-write retry budget was exhausted without finding a free
    /// temp filename. Only plausible under extreme contention or clock skew.
    #[error("atomic write exhausted {attempts} temp-name attempts")]
    AtomicWriteExhausted { attempts: u32 },
}

/// Which operation `rlm setup` should perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupMode {
    /// Apply the rlm configuration, creating or updating as needed.
    Apply,
    /// Dry-run: report what would change, write nothing to disk.
    Check,
    /// Remove all rlm entries from settings and the CLAUDE.local.md block.
    Remove,
}

/// Outcome of a single setup step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupAction {
    /// File or entry did not exist; we created it.
    Created,
    /// File or entry existed; we merged/updated it.
    Updated,
    /// Already in the desired state; no change.
    Skipped,
    /// Entry existed and was removed (only valid in `Remove` mode).
    Removed,
    /// `Check` mode: would create.
    WouldCreate,
    /// `Check` mode: would update.
    WouldUpdate,
    /// `Check` mode: would remove.
    WouldRemove,
    /// Entry was not present to begin with (e.g. `Remove` on a clean project).
    NotPresent,
}

/// Aggregate result of `run_setup`.
#[derive(Debug, Clone, Serialize)]
pub struct SetupReport {
    pub settings_json: SetupAction,
    pub claude_local_md: SetupAction,
    pub initial_index: SetupAction,
}

/// Orchestrate all setup steps for the given mode.
// qual:allow(iosp) reason: "integration: dispatches to the three setup steps"
pub fn run_setup(project_dir: &Path, mode: SetupMode) -> Result<SetupReport> {
    let settings_json = settings::setup_settings_json(project_dir, mode)?;
    let claude_local_md = claude_md::setup_claude_local_md(project_dir, mode)?;
    let initial_index = setup_initial_index(project_dir, mode)?;
    Ok(SetupReport {
        settings_json,
        claude_local_md,
        initial_index,
    })
}

/// Run `rlm index` if the index database is missing.
///
/// In `Remove` mode we preserve the index (it's data, not config) and return
/// `Skipped`. In `Check` mode we report what would happen without writing.
// qual:allow(iosp) reason: "integration: mode dispatch + existence check + index run"
pub fn setup_initial_index(project_dir: &Path, mode: SetupMode) -> Result<SetupAction> {
    let config = Config::new(project_dir);
    let index_exists = config.index_exists();
    match mode {
        SetupMode::Remove => Ok(SetupAction::Skipped),
        SetupMode::Check => {
            if index_exists {
                Ok(SetupAction::Skipped)
            } else {
                Ok(SetupAction::WouldCreate)
            }
        }
        SetupMode::Apply => {
            if index_exists {
                Ok(SetupAction::Skipped)
            } else {
                crate::indexer::run_index(&config, None)?;
                Ok(SetupAction::Created)
            }
        }
    }
}

/// Upper bound on temp-filename collision retries (pid + nanos + counter).
/// Collisions are effectively impossible within this budget in practice.
const MAX_TEMP_ATTEMPTS: u32 = 128;

/// Atomic write via `O_EXCL`-style tempfile + rename.
///
/// Uses `OpenOptions::create_new` so we never follow a pre-existing symlink
/// or overwrite an attacker-seeded file at the temp path. Retries with a
/// monotonic counter suffix if the chosen temp name is already taken.
/// Cross-platform replace: Unix `rename` overwrites atomically; on Windows
/// we remove the target first (see `replace_file`).
// qual:allow(iosp) reason: "retry loop with early-exit on success is inherent to atomic-write-with-collision-retry; per-attempt work is extracted to try_write_once"
pub(super) fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    for attempt in 0..MAX_TEMP_ATTEMPTS {
        let temp = parent.join(format!(
            ".rlm_setup_tmp_{}_{}_{}",
            std::process::id(),
            now_nanos,
            attempt
        ));
        if try_write_once(&temp, path, content)? {
            return Ok(());
        }
    }
    Err(SetupError::AtomicWriteExhausted {
        attempts: MAX_TEMP_ATTEMPTS,
    }
    .into())
}

/// One attempt at atomic write. Returns `Ok(true)` on success, `Ok(false)` if
/// the temp name already existed (caller retries), `Err` on any other failure.
// qual:allow(iosp) reason: "single-attempt atomic write — O_EXCL open + write + rename form one atomic primitive that can't be meaningfully split further"
fn try_write_once(temp: &Path, target: &Path, content: &[u8]) -> Result<bool> {
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(content) {
                drop(file);
                let _ = std::fs::remove_file(temp);
                return Err(e.into());
            }
            drop(file);
            if let Err(e) = replace_file(temp, target) {
                let _ = std::fs::remove_file(temp);
                return Err(e.into());
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Cross-platform file replacement: Unix `rename` atomically overwrites,
/// Windows `rename` requires explicit target removal first.
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if to.exists() {
            std::fs::remove_file(to)?;
        }
    }
    std::fs::rename(from, to)
}
