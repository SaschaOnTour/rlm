//! Runtime-lifecycle CLI commands.
//!
//! Lives in `cli_helpers` (not the `cli` adapter layer) because these
//! commands don't and shouldn't delegate to `application` — they're
//! pure adapter-side wiring:
//!
//! - `cmd_mcp` switches the process from CLI mode into long-running
//!   MCP server mode. Pure runtime infrastructure.
//! - `cmd_setup` writes Claude Code integration files
//!   (`.claude/settings.json`, `CLAUDE.local.md`). That's
//!   integration glue for one specific agent, not core rlm domain.
//!
//! Keeping these out of the `cli` layer avoids false `call_parity`
//! findings without any suppression — the rule applies to command
//! handlers that wrap business logic, and these are not those.

use crate::cli::helpers::{map_err, CmdResult};
use crate::output::{self, Formatter};

pub fn cmd_mcp() -> CmdResult {
    let rt = tokio::runtime::Runtime::new().map_err(map_err)?;
    rt.block_on(async {
        crate::mcp::server::start_mcp_server()
            .await
            .map_err(map_err)
    })
}

pub fn cmd_setup(check: bool, remove: bool, formatter: Formatter) -> CmdResult {
    let mode = if remove {
        crate::interface::cli::setup::SetupMode::Remove
    } else if check {
        crate::interface::cli::setup::SetupMode::Check
    } else {
        crate::interface::cli::setup::SetupMode::Apply
    };
    let cwd = std::env::current_dir().map_err(map_err)?;
    let report = crate::interface::cli::setup::run_setup(&cwd, mode).map_err(map_err)?;
    output::print(formatter, &report);
    Ok(())
}
