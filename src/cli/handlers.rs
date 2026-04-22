//! CLI handlers for code-exploration and edit commands.
//!
//! Every handler in this module is a thin wrapper: parse CLI flags,
//! call one [`RlmSession`] method, emit through the [`Formatter`].
//! All business logic — DB access, staleness refresh, savings
//! bookkeeping, envelope splicing — lives behind `RlmSession` in
//! the application layer.

use crate::application::content::partition;
use crate::application::edit::inserter::InsertPosition;
use crate::application::edit::write_dispatch::{
    DeleteInput, ExtractInput, InsertInput, ReplaceInput,
};
use crate::application::query::read::ReadSymbolInput;
use crate::application::query::search::FieldsMode;
use crate::application::query::DetailLevel;
use crate::application::session::RlmSession;
use crate::cli::commands::{DetailArg, FieldsArg};
use crate::cli::helpers::{map_err, print_str, CmdResult};
use crate::output::{self, Formatter};

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
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session.search(query, limit, mode).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

// qual:allow(srp_params) reason: "path, symbol, parent, section, metadata, formatter are 6 orthogonal CLI args"
pub fn cmd_read(
    path: &str,
    symbol: Option<&str>,
    parent: Option<&str>,
    section: Option<&str>,
    metadata: bool,
    formatter: Formatter,
) -> CmdResult {
    match (symbol, section) {
        (Some(sym), _) => cmd_read_symbol(path, sym, parent, metadata, formatter),
        (_, Some(heading)) => cmd_read_section(path, heading, formatter),
        _ => Err(map_err(
            "read requires --symbol or --section. Use Claude Code's Read for full files or line ranges.",
        )),
    }
}

fn cmd_read_symbol(
    path: &str,
    sym: &str,
    parent: Option<&str>,
    metadata: bool,
    formatter: Formatter,
) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session
        .read_symbol(&ReadSymbolInput {
            path,
            symbol: sym,
            parent,
            metadata,
        })
        .map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

fn cmd_read_section(path: &str, heading: &str, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let result = session.read_section(path, heading).map_err(map_err)?;
    match result.into_body_or_error() {
        Ok(body) => {
            print_str(formatter, &body);
            Ok(())
        }
        Err(msg) => Err(map_err(msg)),
    }
}

pub fn cmd_overview(detail: DetailArg, path: Option<&str>, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let level = match detail {
        DetailArg::Minimal => DetailLevel::Minimal,
        DetailArg::Standard => DetailLevel::Standard,
        DetailArg::Tree => DetailLevel::Tree,
    };
    let response = session.overview(level, path).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

pub fn cmd_refs(symbol: &str, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session.refs(symbol).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

pub fn cmd_partition(path: &str, strategy: &str, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let parsed: partition::Strategy = strategy.parse().map_err(map_err)?;
    let response = session.partition(path, parsed).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

pub fn cmd_summarize(path: &str, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session.summarize(path).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

pub fn cmd_diff(path: &str, symbol: Option<&str>, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session.diff(path, symbol).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

pub fn cmd_context(symbol: &str, graph: bool, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session.context(symbol, graph).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

pub fn cmd_deps(path: &str, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session.deps(path).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

pub fn cmd_scope(path: &str, line: u32, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session.scope(path, line).map_err(map_err)?;
    print_str(formatter, &response.body);
    Ok(())
}

// ── Write-side commands ─────────────────────────────────────────────

// qual:allow(srp_params) reason: "path, symbol, parent, code, preview, formatter are 6 orthogonal CLI args"
pub fn cmd_replace(
    path: &str,
    symbol: &str,
    parent: Option<&str>,
    code: &str,
    preview: bool,
    formatter: Formatter,
) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let input = ReplaceInput {
        path,
        symbol,
        parent,
        code,
    };

    if preview {
        let diff = session.replace_preview(&input).map_err(map_err)?;
        output::print(formatter, &diff);
    } else {
        let result_json = session.replace_apply(&input).map_err(map_err)?;
        print_str(formatter, &result_json);
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
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let result_json = session
        .delete(&DeleteInput {
            path,
            symbol,
            parent,
            keep_docs,
        })
        .map_err(map_err)?;
    print_str(formatter, &result_json);
    Ok(())
}

pub fn cmd_insert(
    path: &str,
    code: &str,
    position: &InsertPosition,
    formatter: Formatter,
) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let result_json = session
        .insert(&InsertInput {
            path,
            position,
            code,
        })
        .map_err(map_err)?;
    print_str(formatter, &result_json);
    Ok(())
}

pub fn cmd_extract(
    path: &str,
    symbols: &[String],
    to: &str,
    parent: Option<&str>,
    formatter: Formatter,
) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let result_json = session
        .extract(&ExtractInput {
            path,
            symbols,
            to,
            parent,
        })
        .map_err(map_err)?;
    print_str(formatter, &result_json);
    Ok(())
}
