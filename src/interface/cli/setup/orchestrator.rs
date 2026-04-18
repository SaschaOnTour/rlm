//! Top-level orchestration for `rlm setup`.
//!
//! Owns the shared public types (`SetupMode`, `SetupAction`, `SetupReport`,
//! `SetupError`), the `run_setup` entrypoint that dispatches to the
//! per-concern sub-modules, and the `setup_initial_index` step. The
//! atomic-write primitives that used to live here were hoisted into
//! `infrastructure::filesystem::atomic_writer` in slice 5.2 so
//! `edit::validator` and `setup` share one implementation.

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
