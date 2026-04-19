//! `rlm setup` — automate Claude Code integration.
//!
//! Handles three concerns for the project under `project_dir`:
//! 1. `.claude/settings.json` — add rlm permissions + `mcpServers.rlm` entry,
//!    preserving any existing user config via array-merge with dedup.
//! 2. `CLAUDE.local.md` — insert a delimited rlm workflow block between
//!    `<!-- rlm:begin -->` / `<!-- rlm:end -->` markers. Preserves content
//!    outside the markers; re-running updates the block in place.
//! 3. Initial index — run `rlm index` if `.rlm/index.db` is missing.
//!
//! No PostToolUse hook is installed: the self-healing index
//! (`crate::application::index::staleness`) picks up external edits at each tool call.
//!
//! Slice 5.1 split the module into three sub-modules:
//! - `orchestrator` owns the user-facing `run_setup` + the shared
//!   `SetupMode` / `SetupAction` / `SetupReport` / `SetupError` types.
//! - `settings` implements `.claude/settings.json` merge/strip.
//! - `claude_md` implements the `CLAUDE.local.md` marker-block upsert.
//!
//! The atomic-write primitives moved to
//! `crate::infrastructure::filesystem::atomic_writer` in slice 5.2 and
//! `settings` / `claude_md` call that module directly.

mod claude_md;
mod orchestrator;
mod settings;

pub use claude_md::setup_claude_local_md;
pub use orchestrator::{
    run_setup, setup_initial_index, SetupAction, SetupError, SetupMode, SetupReport,
};
pub use settings::{merge_settings, rlm_defaults, setup_settings_json, strip_rlm_from_settings};
