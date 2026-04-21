//! CLI handlers for code-exploration and edit commands.
//!
//! Every handler in this module is a thin wrapper: parse CLI flags,
//! call one [`RlmSession`] method, emit through the [`Formatter`].
//! All business logic — DB access, staleness refresh, savings
//! bookkeeping, envelope splicing — lives behind `RlmSession` in
//! the application layer.

use crate::application::edit::inserter::InsertPosition;
use crate::application::edit::write_dispatch::{
    DeleteInput, ExtractInput, InsertInput, ReplaceInput,
};
use crate::application::query::read::{ReadSectionResult, ReadSymbolInput, MAX_SECTION_HINT};
use crate::application::query::search::FieldsMode;
use crate::application::session::RlmSession;
use crate::cli::commands::FieldsArg;
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
    match session.read_section(path, heading).map_err(map_err)? {
        ReadSectionResult::Found { body, .. } => {
            print_str(formatter, &body);
            Ok(())
        }
        ReadSectionResult::NotFound {
            heading,
            available,
            total,
        } => Err(map_err(format_section_not_found(
            &heading, &available, total,
        ))),
        ReadSectionResult::FileNotFound { path } => Err(map_err(format!("file not found: {path}"))),
    }
}

fn format_section_not_found(heading: &str, available: &[String], total: usize) -> String {
    if available.is_empty() {
        return format!("section not found: {heading}. File has no sections.");
    }
    if total > available.len() {
        format!(
            "section not found: {heading}. Available ({total} total, first {MAX_SECTION_HINT}): {}",
            available.join(", ")
        )
    } else {
        format!(
            "section not found: {heading}. Available: {}",
            available.join(", ")
        )
    }
}

pub fn cmd_overview(detail: &str, path: Option<&str>, formatter: Formatter) -> CmdResult {
    let session = RlmSession::open_cwd().map_err(map_err)?;
    let response = session.overview(detail, path).map_err(map_err)?;
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
    let response = session.partition(path, strategy).map_err(map_err)?;
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
