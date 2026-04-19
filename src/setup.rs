//! `rlm setup` — automate Claude Code integration.
//!
//! Implementation moved to `crate::interface::cli::setup` sub-modules in
//! slice 5.1; this file is a thin re-export bridge so existing call sites
//! (`crate::setup::...`, `rlm::setup::...` in integration tests) keep
//! compiling. Prefer importing from `crate::interface::cli::setup` in new
//! code.

pub use crate::interface::cli::setup::{
    merge_settings, rlm_defaults, run_setup, setup_claude_local_md, setup_initial_index,
    setup_settings_json, strip_rlm_from_settings, SetupAction, SetupError, SetupMode, SetupReport,
};
