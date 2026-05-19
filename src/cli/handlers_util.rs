//! CLI handlers for system/utility commands.
//!
//! Code-exploration commands live in `cli::handlers`.
//! Shared helpers live in `cli::helpers`.
//!
//! Each handler parses its CLI flags, makes a single
//! [`application::facades`](crate::application::facades) call against
//! the cwd-discovered project root, and emits the result through the
//! formatter — that's the call_parity contract both adapters honour.
//!
//! Two documented exceptions deliberately skip the facade seam:
//! - `cmd_files` calls [`application::query::files::list_files`]
//!   directly. `files` is filesystem-backed and must not trigger
//!   `ensure_index`; a session would auto-index on first read.
//!   `handle_files` on the MCP side uses the same direct path.
//! - `cmd_supported` calls [`RlmSession::supported`] directly. The
//!   list of supported languages is a pure function — no project
//!   root, no session state. `handle_supported` on MCP mirrors this.

use crate::application::facades;
use crate::application::query::files::FilesFilter;
use crate::application::query::stats::QualityFlags;
use crate::application::session::RlmSession;
use crate::cli::helpers::{cwd_project_root, map_err, CmdResult};
use crate::output::{self, Formatter};

pub fn cmd_stats(show_savings: bool, since: Option<&str>, formatter: Formatter) -> CmdResult {
    let root = cwd_project_root()?;
    let out = facades::stats_project(&root, show_savings, since).map_err(map_err)?;
    output::print(formatter, &out.body);

    // Quality sidechannel (stderr): only populated on the stats path,
    // never on the savings path, so the primary stdout envelope stays
    // a single machine-readable JSON document.
    if let Some(sidechannel) = out.quality_sidechannel {
        eprintln!("{}", formatter.serialize(&sidechannel));
    }
    Ok(())
}

pub fn cmd_quality(
    unknown_only: bool,
    all: bool,
    summary: bool,
    formatter: Formatter,
) -> CmdResult {
    let root = cwd_project_root()?;
    let body = facades::quality_project(
        &root,
        QualityFlags {
            unknown_only,
            all,
            summary,
        },
    )
    .map_err(map_err)?;
    output::print(formatter, &body);
    Ok(())
}

pub fn cmd_quality_clear(formatter: Formatter) -> CmdResult {
    let root = cwd_project_root()?;
    let ack = facades::quality_clear_project(&root).map_err(map_err)?;
    output::print(formatter, &ack);
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
    let root = cwd_project_root()?;
    let filter = FilesFilter {
        path_prefix: path_filter.map(String::from),
        skipped_only,
        indexed_only,
    };
    let result = crate::application::query::files::list_files(&root, filter).map_err(map_err)?;
    output::print(formatter, &result);
    Ok(())
}

pub fn cmd_verify(fix: bool, formatter: Formatter) -> CmdResult {
    let root = cwd_project_root()?;
    let result = facades::verify_project(&root, fix).map_err(map_err)?;
    output::print(formatter, &result);
    Ok(())
}

pub fn cmd_supported(formatter: Formatter) -> CmdResult {
    output::print(formatter, &RlmSession::supported());
    Ok(())
}
