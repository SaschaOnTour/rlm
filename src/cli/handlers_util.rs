//! CLI handlers for system/utility commands.
//!
//! Code-exploration commands live in `cli::handlers`.
//! Shared helpers live in `cli::helpers`.
//!
//! Every handler is a thin adapter over [`RlmSession`]: parse CLI
//! flags, call one session method, emit through the formatter.

use crate::application::query::files::FilesFilter;
use crate::application::query::stats::QualityFlags;
use crate::application::session::RlmSession;
use crate::cli::helpers::{map_err, CmdResult};
use crate::config::Config;
use crate::output::{self, Formatter};

pub fn cmd_stats(show_savings: bool, since: Option<&str>, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let out = session.stats(show_savings, since).map_err(map_err)?;
    output::print(formatter, &out.body);

    // Quality sidechannel (stderr): only populated on the stats path,
    // never on the savings path, so the primary stdout envelope stays
    // a single machine-readable JSON document.
    if let Some(sidechannel) = out.quality_sidechannel {
        eprintln!("{}", formatter.serialize(&sidechannel));
    }
    Ok(())
}

pub fn cmd_mcp() -> CmdResult {
    let rt = tokio::runtime::Runtime::new().map_err(map_err)?;
    rt.block_on(async {
        crate::mcp::server::start_mcp_server()
            .await
            .map_err(map_err)
    })
}

pub fn cmd_quality(
    unknown_only: bool,
    all: bool,
    clear: bool,
    summary: bool,
    formatter: Formatter,
) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let body = session
        .quality(QualityFlags {
            unknown_only,
            all,
            clear,
            summary,
        })
        .map_err(map_err)?;
    output::print(formatter, &body);
    Ok(())
}

pub fn cmd_files(
    path_filter: Option<&str>,
    skipped_only: bool,
    indexed_only: bool,
    formatter: Formatter,
) -> CmdResult {
    // `files` is filesystem-backed and must not trigger an index
    // build — `RlmSession::open_cwd` would call `ensure_index`,
    // which is expensive on a fresh project. MCP's `handle_files`
    // uses the same direct path.
    let config = Config::from_cwd().map_err(map_err)?;
    let filter = FilesFilter {
        path_prefix: path_filter.map(String::from),
        skipped_only,
        indexed_only,
    };
    let result = crate::application::query::files::list_files(&config.project_root, filter)
        .map_err(map_err)?;
    output::print(formatter, &result);
    Ok(())
}

pub fn cmd_verify(fix: bool, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let result = session.verify(fix).map_err(map_err)?;
    output::print(formatter, &result);
    Ok(())
}

pub fn cmd_supported(formatter: Formatter) -> CmdResult {
    output::print(formatter, &RlmSession::supported());
    Ok(())
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
