//! CLI handlers for code-exploration and edit commands.
//!
//! Every handler in this module is a thin wrapper: parse CLI flags,
//! call exactly **one** [`facades`] function (the per-command
//! application entry point), emit the result through the
//! [`Formatter`]. All business logic — DB access, staleness refresh,
//! savings bookkeeping, envelope splicing — lives behind the facades
//! in the application layer.

use crate::application::content::partition;
use crate::application::edit::inserter::InsertPosition;
use crate::application::edit::write_dispatch::{
    DeleteInput, ExtractInput, InsertInput, ReplaceInput, ReplaceMode, ReplaceOutput,
};
use crate::application::facades;
use crate::application::query::read::ReadInputs;
use crate::application::query::search::FieldsMode;
use crate::application::query::DetailLevel;
use crate::application::session::RlmSession;
use crate::cli::commands::{CodeSource, DetailArg, FieldsArg};
use crate::cli::helpers::{cwd_project_root, map_err, resolve_code, run_facade, CmdResult};
use crate::output::{self, print_str, Formatter};

// ── Read-side commands ──────────────────────────────────────────────

pub fn cmd_index(path: &str, formatter: Formatter) -> CmdResult {
    // `.` means "use cwd"; any other value is taken as given.
    let root = if path == "." {
        std::env::current_dir().map_err(map_err)?
    } else {
        std::path::PathBuf::from(path)
    };

    let progress = |current: usize, total: usize| {
        if current.is_multiple_of(output::PROGRESS_INTERVAL) || current == total {
            eprint!("\rIndexing... {current}/{total} files");
        }
    };
    let result = RlmSession::index_project(&root, Some(&progress)).map_err(map_err)?;
    if result.files_scanned > 0 {
        eprintln!();
    }
    output::print(formatter, &result);
    Ok(())
}

pub fn cmd_search(query: &str, limit: usize, fields: FieldsArg, formatter: Formatter) -> CmdResult {
    let mode = match fields {
        FieldsArg::Full => FieldsMode::Full,
        FieldsArg::Minimal => FieldsMode::Minimal,
    };
    run_facade(formatter, |root| {
        facades::search_project(root, query, limit, mode).map(|r| r.body)
    })
}

/// Grouped clap inputs for [`cmd_read`]. Carries the raw flags from
/// the parser; the dispatch into [`ReadRequest::Symbol`] /
/// [`ReadRequest::Section`] happens inside the handler.
pub struct ReadCliArgs<'a> {
    pub path: &'a str,
    pub symbol: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub section: Option<&'a str>,
    pub metadata: bool,
}

pub fn cmd_read(args: &ReadCliArgs<'_>, formatter: Formatter) -> CmdResult {
    let inputs = ReadInputs {
        path: args.path,
        symbol: args.symbol,
        section: args.section,
        parent: args.parent,
        metadata: args.metadata,
    };
    run_facade(formatter, |root| {
        facades::read_project(root, &inputs).map(|r| r.body)
    })
}

pub fn cmd_overview(detail: DetailArg, path: Option<&str>, formatter: Formatter) -> CmdResult {
    let level = match detail {
        DetailArg::Minimal => DetailLevel::Minimal,
        DetailArg::Standard => DetailLevel::Standard,
        DetailArg::Tree => DetailLevel::Tree,
    };
    run_facade(formatter, |root| {
        facades::overview_project(root, level, path).map(|r| r.body)
    })
}

pub fn cmd_refs(symbol: &str, parent: Option<&str>, formatter: Formatter) -> CmdResult {
    run_facade(formatter, |root| {
        facades::refs_project(root, symbol, parent).map(|r| r.body)
    })
}

pub fn cmd_partition(path: &str, strategy: &str, formatter: Formatter) -> CmdResult {
    let parsed: partition::Strategy = strategy.parse().map_err(map_err)?;
    run_facade(formatter, |root| {
        facades::partition_project(root, path, parsed).map(|r| r.body)
    })
}

pub fn cmd_summarize(path: &str, formatter: Formatter) -> CmdResult {
    run_facade(formatter, |root| {
        facades::summarize_project(root, path).map(|r| r.body)
    })
}

pub fn cmd_diff(path: &str, symbol: Option<&str>, formatter: Formatter) -> CmdResult {
    run_facade(formatter, |root| {
        facades::diff_project(root, path, symbol).map(|r| r.body)
    })
}

pub fn cmd_context(symbol: &str, graph: bool, formatter: Formatter) -> CmdResult {
    run_facade(formatter, |root| {
        facades::context_project(root, symbol, graph).map(|r| r.body)
    })
}

pub fn cmd_deps(path: &str, formatter: Formatter) -> CmdResult {
    run_facade(formatter, |root| {
        facades::deps_project(root, path).map(|r| r.body)
    })
}

pub fn cmd_scope(path: &str, line: u32, formatter: Formatter) -> CmdResult {
    run_facade(formatter, |root| {
        facades::scope_project(root, path, line).map(|r| r.body)
    })
}

// ── Write-side commands ─────────────────────────────────────────────

/// Grouped clap inputs for [`cmd_replace`]. The `preview` flag maps
/// to [`ReplaceMode`] inside the handler.
pub struct ReplaceCliArgs<'a> {
    pub path: &'a str,
    pub symbol: &'a str,
    pub parent: Option<&'a str>,
    pub code_source: &'a CodeSource,
    pub preview: bool,
}

pub fn cmd_replace(args: &ReplaceCliArgs<'_>, formatter: Formatter) -> CmdResult {
    let code = resolve_code(args.code_source)?;
    let input = ReplaceInput {
        path: args.path,
        symbol: args.symbol,
        parent: args.parent,
        code: &code,
    };
    let mode = if args.preview {
        ReplaceMode::Preview
    } else {
        ReplaceMode::Apply
    };
    let root = cwd_project_root()?;
    match facades::replace_project(&root, &input, mode).map_err(map_err)? {
        ReplaceOutput::Preview(diff) => output::print(formatter, &diff),
        ReplaceOutput::Applied(json) => print_str(formatter, &json),
    }
    Ok(())
}

pub fn cmd_delete(
    path: &str,
    symbol: &str,
    parent: Option<&str>,
    keep_docs: bool,
    formatter: Formatter,
) -> CmdResult {
    run_facade(formatter, |root| {
        facades::delete_project(
            root,
            &DeleteInput {
                path,
                symbol,
                parent,
                keep_docs,
            },
        )
    })
}

pub fn cmd_insert(
    path: &str,
    code_source: &CodeSource,
    position: &InsertPosition,
    formatter: Formatter,
) -> CmdResult {
    let code = resolve_code(code_source)?;
    run_facade(formatter, |root| {
        facades::insert_project(
            root,
            &InsertInput {
                path,
                position,
                code: &code,
            },
        )
    })
}

pub fn cmd_extract(
    path: &str,
    symbols: &[String],
    to: &str,
    parent: Option<&str>,
    formatter: Formatter,
) -> CmdResult {
    run_facade(formatter, |root| {
        facades::extract_project(
            root,
            &ExtractInput {
                path,
                symbols,
                to,
                parent,
            },
        )
    })
}
