// Inherit lint configuration from lib.rs for consistency
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::fn_params_excessive_bools,
    clippy::unnecessary_wraps,
    clippy::match_same_arms
)]

use clap::Parser;

use rlm::cli::commands::{Cli, Command};
use rlm::cli::handlers;
use rlm::cli::handlers_util;
use rlm::cli::output;

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("{}", output::format_error(&e));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::fmt::Display>> {
    match cli.command {
        Command::Index { path } => handlers::cmd_index(&path),
        Command::Search { query, limit } => handlers::cmd_search(&query, limit),
        Command::Read {
            path,
            symbol,
            section,
            metadata,
        } => handlers::cmd_read(&path, symbol.as_deref(), section.as_deref(), metadata),
        Command::Overview { detail, path } => handlers::cmd_overview(&detail, path.as_deref()),
        Command::Refs { symbol } => handlers::cmd_refs(&symbol),
        Command::Replace {
            path,
            symbol,
            code,
            preview,
        } => handlers::cmd_replace(&path, &symbol, &code, preview),
        Command::Insert {
            path,
            code,
            position,
        } => handlers::cmd_insert(&path, &code, &position),
        Command::Stats { savings, since } => handlers_util::cmd_stats(savings, since.as_deref()),
        Command::Partition { path, strategy } => handlers::cmd_partition(&path, &strategy),
        Command::Summarize { path } => handlers::cmd_summarize(&path),
        Command::Diff { path, symbol } => handlers::cmd_diff(&path, symbol.as_deref()),
        Command::Context { symbol, graph } => handlers::cmd_context(&symbol, graph),
        Command::Deps { path } => handlers::cmd_deps(&path),
        Command::Scope { path, line } => handlers::cmd_scope(&path, line),
        Command::Mcp => handlers_util::cmd_mcp(),
        Command::Quality {
            unknown_only,
            all,
            clear,
            summary,
        } => handlers_util::cmd_quality(unknown_only, all, clear, summary),
        Command::Files {
            path,
            skipped_only,
            indexed_only,
        } => handlers_util::cmd_files(path.as_deref(), skipped_only, indexed_only),
        Command::Verify { fix } => handlers_util::cmd_verify(fix),
        Command::Supported => handlers_util::cmd_supported(),
    }
}
