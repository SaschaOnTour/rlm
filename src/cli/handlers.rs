//! CLI handlers for code-exploration and edit commands.
//!
//! System/utility commands live in `cli::handlers_util`.
//! Shared helpers live in `cli::helpers`.

use crate::cli::helpers::{
    cmd_single_file_op, emit_read_symbol, format_chunks, get_config, get_db, map_err,
    parse_strategy, print_str, print_write_result, CmdResult,
};
use crate::domain::token_budget::estimate_json_tokens;
use crate::edit::inserter::InsertPosition;
use crate::edit::syntax_guard::SyntaxGuard;
use crate::edit::{inserter, replacer};
use crate::indexer;
use crate::operations;
use crate::operations::savings;
use crate::output::{self, Formatter};
use crate::rlm::{partition, peek, summarize};
use crate::search::tree;

pub fn cmd_index(path: &str, formatter: Formatter) -> CmdResult {
    let config = if path == "." {
        get_config()?
    } else {
        crate::config::Config::new(path)
    };

    let progress = |current: usize, total: usize| {
        if current.is_multiple_of(output::PROGRESS_INTERVAL) || current == total {
            eprint!("\rIndexing... {current}/{total} files");
        }
    };
    let result = crate::indexer::run_index(&config, Some(&progress)).map_err(map_err)?;
    if result.files_scanned > 0 {
        eprintln!();
    }
    let output: operations::IndexOutput = result.into();
    formatter.print(&output);
    Ok(())
}

pub fn cmd_search(query: &str, limit: usize, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::search_chunks(&db, query, limit).map_err(map_err)?;
    // Estimate tokens from JSON (savings always tracks JSON cost, regardless of output format)
    let json_for_savings = output::to_json(&result);
    let out_tokens = estimate_json_tokens(json_for_savings.len());
    let file_count = result.results.len() as u64;
    let alt_tokens = result.tokens.output.max(out_tokens);
    savings::record(&db, "search", out_tokens, alt_tokens, file_count);
    formatter.print(&result);
    Ok(())
}

pub fn cmd_read(
    path: &str,
    symbol: Option<&str>,
    section: Option<&str>,
    metadata: bool,
    formatter: Formatter,
) -> CmdResult {
    match (symbol, section) {
        (Some(sym), _) => cmd_read_symbol(path, sym, metadata, formatter),
        (_, Some(heading)) => cmd_read_section(path, heading, formatter),
        _ => Err(map_err(
            "read requires --symbol or --section. Use Claude Code's Read for full files or line ranges.",
        )),
    }
}

fn cmd_read_symbol(path: &str, sym: &str, metadata: bool, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    let chunks = db.get_chunks_by_ident(sym).map_err(map_err)?;
    // Single O(1) file lookup instead of get_all_files() per chunk
    let file_id = db.get_file_by_path(path).ok().flatten().map(|f| f.id);
    let file_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| file_id.is_some_and(|fid| c.file_id == fid))
        .collect();

    let target_json = if file_chunks.is_empty() {
        if chunks.is_empty() {
            return Err(map_err(format!("symbol not found: {sym}")));
        }
        serde_json::json!(chunks)
    } else {
        serde_json::json!(file_chunks)
    };

    let json = format_chunks(&db, sym, &target_json, metadata);
    emit_read_symbol(&db, path, &json, formatter);
    Ok(())
}

fn cmd_read_section(path: &str, heading: &str, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    let file = db.get_file_by_path(path).map_err(map_err)?;
    let file = file.ok_or_else(|| map_err(format!("file not found: {path}")))?;
    let chunks = db.get_chunks_for_file(file.id).map_err(map_err)?;
    match chunks
        .iter()
        .find(|c| c.kind.is_section() && c.ident == heading)
    {
        Some(c) => {
            let json = savings::record_file_op(&db, "read_section", c, path);
            print_str(formatter, &json);
        }
        None => return Err(map_err(format!("section not found: {heading}"))),
    }
    Ok(())
}

pub fn cmd_overview(detail: &str, path: Option<&str>, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    match detail {
        "minimal" => {
            let result = peek::peek(&db, path).map_err(map_err)?;
            let json = savings::record_scoped_op(&db, "overview", &result, path);
            print_str(formatter, &json);
        }
        "standard" => {
            let entries = operations::build_map(&db, path).map_err(map_err)?;
            let json = savings::record_scoped_op(&db, "overview", &entries, path);
            print_str(formatter, &json);
        }
        "tree" => {
            let nodes = tree::build_tree(&db, path).map_err(map_err)?;
            let json = savings::record_scoped_op(&db, "overview", &nodes, path);
            print_str(formatter, &json);
        }
        other => {
            return Err(map_err(format!(
                "unknown detail level: '{other}'. Use 'minimal', 'standard', or 'tree'."
            )));
        }
    }
    Ok(())
}

pub fn cmd_refs(symbol: &str, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::analyze_impact(&db, symbol).map_err(map_err)?;
    let json = savings::record_symbol_op(&db, "refs", &result, symbol, result.count as u64);
    print_str(formatter, &json);
    Ok(())
}

pub fn cmd_replace(
    path: &str,
    symbol: &str,
    code: &str,
    preview: bool,
    formatter: Formatter,
) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if preview {
        let diff = replacer::preview_replace(&db, path, symbol, code).map_err(map_err)?;
        formatter.print(&diff);
    } else {
        let outcome = replacer::replace_symbol(&db, path, symbol, code, &config.project_root)
            .map_err(map_err)?;
        let result_json = print_write_result(
            &db,
            &config,
            path,
            indexer::PreviewSource::Symbol(symbol),
            formatter,
        );
        if let Ok(entry) = savings::alternative_replace_entry(
            &db,
            path,
            outcome.old_code_len,
            code.len(),
            result_json.len(),
        ) {
            savings::record_v2(&db, &entry);
        }
    }
    Ok(())
}

pub fn cmd_insert(
    path: &str,
    code: &str,
    position: &InsertPosition,
    formatter: Formatter,
) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let guard = SyntaxGuard::new();
    inserter::insert_code(&config.project_root, path, position, code, &guard).map_err(map_err)?;
    let result_json = print_write_result(&db, &config, path, position.preview_source(), formatter);
    if let Ok(entry) = savings::alternative_insert_entry(&db, path, code.len(), result_json.len()) {
        savings::record_v2(&db, &entry);
    }
    Ok(())
}

pub fn cmd_partition(path: &str, strategy_str: &str, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let strategy = parse_strategy(strategy_str)?;
    let result =
        partition::partition_file(&db, path, &strategy, &config.project_root).map_err(map_err)?;
    let json = savings::record_file_op(&db, "partition", &result, path);
    print_str(formatter, &json);
    Ok(())
}

pub fn cmd_summarize(path: &str, formatter: Formatter) -> CmdResult {
    cmd_single_file_op("summarize", path, summarize::summarize, formatter)
}

pub fn cmd_diff(path: &str, symbol: Option<&str>, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if let Some(sym) = symbol {
        let result =
            operations::diff_symbol(&db, path, sym, &config.project_root).map_err(map_err)?;
        let json = savings::record_file_op(&db, "diff", &result, path);
        print_str(formatter, &json);
    } else {
        let result = operations::diff_file(&db, path, &config.project_root).map_err(map_err)?;
        let json = savings::record_file_op(&db, "diff", &result, path);
        print_str(formatter, &json);
    }
    Ok(())
}

pub fn cmd_context(symbol: &str, graph: bool, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::build_context(&db, symbol).map_err(map_err)?;

    let file_count = result.file_count as u64;
    if graph {
        let callgraph = operations::build_callgraph(&db, symbol).map_err(map_err)?;
        let combined = serde_json::json!({
            "context": result,
            "callgraph": callgraph,
        });
        let json = savings::record_symbol_op(&db, "context", &combined, symbol, file_count);
        print_str(formatter, &json);
    } else {
        let json = savings::record_symbol_op(&db, "context", &result, symbol, file_count);
        print_str(formatter, &json);
    }
    Ok(())
}

pub fn cmd_deps(path: &str, formatter: Formatter) -> CmdResult {
    cmd_single_file_op("deps", path, operations::get_deps, formatter)
}

pub fn cmd_scope(path: &str, line: u32, formatter: Formatter) -> CmdResult {
    cmd_single_file_op(
        "scope",
        path,
        |db, p| operations::get_scope(db, p, line),
        formatter,
    )
}
