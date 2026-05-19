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

use rlm::cli::commands::{self, Cli, Command};
use rlm::cli::handlers;
use rlm::cli::handlers_util;
use rlm::cli::lifecycle_handlers;
use rlm::output::{Formatter, OutputFormat};

fn main() {
    let cli = Cli::parse();
    // MCP server builds its own Formatter from its project config — don't
    // derive one from CLI flags here.
    let formatter = if matches!(cli.command, Command::Mcp) {
        Formatter::default()
    } else {
        let format = match cli.format {
            Some(commands::FormatArg::Pretty) => OutputFormat::Pretty,
            Some(commands::FormatArg::Toon) => OutputFormat::Toon,
            Some(commands::FormatArg::Json) => OutputFormat::Json,
            None => {
                // No explicit --format flag: read format from the command's
                // target config so `rlm index /other/project` respects
                // /other/project/.rlm/config.toml (same root that cmd_index
                // uses for indexing settings).
                let config = match &cli.command {
                    Command::Index { path } => rlm::config::Config::new(path),
                    _ => {
                        let cwd = std::env::current_dir().unwrap_or_default();
                        rlm::config::Config::new(&cwd)
                    }
                };
                OutputFormat::from_str_loose(&config.settings.output.format)
            }
        };
        Formatter::new(format)
    };

    if let Err(e) = run(cli, formatter) {
        eprintln!("{}", formatter.serialize_error(&e));
        std::process::exit(1);
    }
}

fn run(cli: Cli, formatter: Formatter) -> Result<(), Box<dyn std::fmt::Display>> {
    match cli.command {
        Command::Index { path } => handlers::cmd_index(&path, formatter),
        Command::Search {
            query,
            limit,
            fields,
        } => handlers::cmd_search(&query, limit, fields, formatter),
        Command::Read {
            path,
            symbol,
            parent,
            section,
            metadata,
        } => handlers::cmd_read(
            &handlers::ReadCliArgs {
                path: &path,
                symbol: symbol.as_deref(),
                parent: parent.as_deref(),
                section: section.as_deref(),
                metadata,
            },
            formatter,
        ),
        Command::Overview { detail, path } => {
            handlers::cmd_overview(detail, path.as_deref(), formatter)
        }
        Command::Refs { symbol, parent } => {
            handlers::cmd_refs(&symbol, parent.as_deref(), formatter)
        }
        Command::Replace {
            path,
            symbol,
            parent,
            code_source,
            preview,
        } => handlers::cmd_replace(
            &handlers::ReplaceCliArgs {
                path: &path,
                symbol: &symbol,
                parent: parent.as_deref(),
                code_source: &code_source,
                preview,
            },
            formatter,
        ),
        Command::Delete {
            path,
            symbol,
            parent,
            keep_docs,
        } => handlers::cmd_delete(&path, &symbol, parent.as_deref(), keep_docs, formatter),
        Command::Extract {
            path,
            symbols,
            to,
            parent,
        } => handlers::cmd_extract(&path, &symbols, &to, parent.as_deref(), formatter),
        Command::Insert {
            path,
            code_source,
            position,
        } => handlers::cmd_insert(&path, &code_source, &position, formatter),
        Command::Stats { savings, since } => {
            handlers_util::cmd_stats(savings, since.as_deref(), formatter)
        }
        Command::Partition { path, strategy } => {
            handlers::cmd_partition(&path, &strategy, formatter)
        }
        Command::Summarize { path } => handlers::cmd_summarize(&path, formatter),
        Command::Diff { path, symbol } => handlers::cmd_diff(&path, symbol.as_deref(), formatter),
        Command::Context { symbol, graph } => handlers::cmd_context(&symbol, graph, formatter),
        Command::Deps { path } => handlers::cmd_deps(&path, formatter),
        Command::Scope { path, line } => handlers::cmd_scope(&path, line, formatter),
        Command::Mcp => lifecycle_handlers::cmd_mcp(),
        Command::Quality {
            cmd,
            unknown_only,
            all,
            summary,
        } => dispatch_quality(cmd, unknown_only, all, summary, formatter),
        Command::Files {
            path,
            skipped_only,
            indexed_only,
        } => handlers_util::cmd_files(path.as_deref(), skipped_only, indexed_only, formatter),
        Command::Verify { fix } => handlers_util::cmd_verify(fix, formatter),
        Command::Supported => handlers_util::cmd_supported(formatter),
        Command::Setup { check, remove } => lifecycle_handlers::cmd_setup(check, remove, formatter),
    }
}

/// Quality dispatch shim — kept out of `run` so the composition-root
/// match stays under the `LONG_FN` threshold. Encapsulates the
/// inspect/clear split that lives on the CLI as `rlm quality [clear]`
/// and on MCP as `quality` + `quality_clear`.
fn dispatch_quality(
    cmd: Option<commands::QualityCmd>,
    unknown_only: bool,
    all: bool,
    summary: bool,
    formatter: Formatter,
) -> Result<(), Box<dyn std::fmt::Display>> {
    match cmd {
        Some(commands::QualityCmd::Clear) => handlers_util::cmd_quality_clear(formatter),
        None => handlers_util::cmd_quality(unknown_only, all, summary, formatter),
    }
}
