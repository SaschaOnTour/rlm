//! CLI handlers for code-exploration and edit commands.
//!
//! System/utility commands live in `cli::handlers_util`.
//! Shared helpers live in `cli::helpers`.

use crate::cli::helpers::{
    cmd_single_file_op, emit_read_symbol, format_chunks_json, get_config, get_db, map_err,
    parse_strategy, print_json, CmdResult,
};
use crate::cli::output;
use crate::edit::inserter::InsertPosition;
use crate::edit::syntax_guard::SyntaxGuard;
use crate::edit::{inserter, replacer};
use crate::models::token_estimate::estimate_tokens;
use crate::operations;
use crate::operations::savings;
use crate::rlm::{partition, peek, summarize};
use crate::search::tree;

pub fn cmd_index(path: &str) -> CmdResult {
    let config = if path == "." {
        get_config()?
    } else {
        crate::config::Config::new(path)
    };

    let result = crate::indexer::run_index(&config).map_err(map_err)?;
    let output: operations::IndexOutput = result.into();
    println!("{}", output::format_json(&output));
    Ok(())
}

pub fn cmd_search(query: &str, limit: usize) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::search_chunks(&db, query, limit).map_err(map_err)?;
    let json = output::format_json(&result);
    let out_tokens = estimate_tokens(json.len());
    let file_count = result.results.len() as u64;
    let alt_tokens = result.tokens.output.max(out_tokens);
    savings::record(&db, "search", out_tokens, alt_tokens, file_count);
    print_json(&json);
    Ok(())
}

pub fn cmd_read(
    path: &str,
    symbol: Option<&str>,
    section: Option<&str>,
    metadata: bool,
) -> CmdResult {
    match (symbol, section) {
        (Some(sym), _) => cmd_read_symbol(path, sym, metadata),
        (_, Some(heading)) => cmd_read_section(path, heading),
        _ => Err(map_err(
            "read requires --symbol or --section. Use Claude Code's Read for full files or line ranges.",
        )),
    }
}

fn cmd_read_symbol(path: &str, sym: &str, metadata: bool) -> CmdResult {
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

    let json = format_chunks_json(&db, sym, &target_json, metadata);
    emit_read_symbol(&db, path, &json);
    Ok(())
}

fn cmd_read_section(path: &str, heading: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    let file = db.get_file_by_path(path).map_err(map_err)?;
    let file = file.ok_or_else(|| map_err(format!("file not found: {path}")))?;
    let chunks = db.get_chunks_for_file(file.id).map_err(map_err)?;
    let section_chunk = chunks.iter().find(|c| c.ident == heading);
    match section_chunk {
        Some(c) => {
            let json = output::format_json(c);
            let out_tokens = estimate_tokens(json.len());
            let alt_tokens = savings::alternative_single_file(&db, path).unwrap_or(out_tokens);
            savings::record(&db, "read_section", out_tokens, alt_tokens, 1);
            print_json(&json);
        }
        None => return Err(map_err(format!("section not found: {heading}"))),
    }
    Ok(())
}

pub fn cmd_overview(detail: &str, path: Option<&str>) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    match detail {
        "minimal" => {
            let result = peek::peek(&db, path).map_err(map_err)?;
            let json = savings::record_scoped_op(&db, "overview", &result, path);
            print_json(&json);
        }
        "standard" => {
            let entries = operations::build_map(&db, path).map_err(map_err)?;
            let json = savings::record_scoped_op(&db, "overview", &entries, path);
            print_json(&json);
        }
        "tree" => {
            let nodes = tree::build_tree(&db, path).map_err(map_err)?;
            let json = savings::record_scoped_op(&db, "overview", &nodes, path);
            print_json(&json);
        }
        other => {
            return Err(map_err(format!(
                "unknown detail level: '{other}'. Use 'minimal', 'standard', or 'tree'."
            )));
        }
    }
    Ok(())
}

pub fn cmd_refs(symbol: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::analyze_impact(&db, symbol).map_err(map_err)?;
    let json = savings::record_symbol_op(&db, "refs", &result, symbol, result.count as u64);
    print_json(&json);
    Ok(())
}

pub fn cmd_replace(path: &str, symbol: &str, code: &str, preview: bool) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if preview {
        let diff = replacer::preview_replace(&db, path, symbol, code).map_err(map_err)?;
        print_json(&output::format_json(&diff));
    } else {
        replacer::replace_symbol(&db, path, symbol, code, &config.project_root).map_err(map_err)?;
        let _ = crate::indexer::reindex_single_file(&db, &config, path);
        println!("{{\"ok\":true}}");
    }
    Ok(())
}

pub fn cmd_insert(path: &str, code: &str, position: &InsertPosition) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let guard = SyntaxGuard::new();
    inserter::insert_code(&config.project_root, path, position, code, &guard).map_err(map_err)?;
    let _ = crate::indexer::reindex_single_file(&db, &config, path);
    println!("{{\"ok\":true}}");
    Ok(())
}

pub fn cmd_partition(path: &str, strategy_str: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let strategy = parse_strategy(strategy_str)?;
    let result =
        partition::partition_file(&db, path, &strategy, &config.project_root).map_err(map_err)?;
    let json = savings::record_file_op(&db, "partition", &result, path);
    print_json(&json);
    Ok(())
}

pub fn cmd_summarize(path: &str) -> CmdResult {
    cmd_single_file_op("summarize", path, summarize::summarize)
}

pub fn cmd_diff(path: &str, symbol: Option<&str>) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if let Some(sym) = symbol {
        let result =
            operations::diff_symbol(&db, path, sym, &config.project_root).map_err(map_err)?;
        let json = savings::record_file_op(&db, "diff", &result, path);
        print_json(&json);
    } else {
        let result = operations::diff_file(&db, path, &config.project_root).map_err(map_err)?;
        let json = savings::record_file_op(&db, "diff", &result, path);
        print_json(&json);
    }
    Ok(())
}

pub fn cmd_context(symbol: &str, graph: bool) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::build_context(&db, symbol).map_err(map_err)?;

    if graph {
        let callgraph = operations::build_callgraph(&db, symbol).map_err(map_err)?;
        let combined = serde_json::json!({
            "context": result,
            "callgraph": callgraph,
        });
        let json = savings::record_symbol_op(&db, "context", &combined, symbol, 0);
        print_json(&json);
    } else {
        let json = savings::record_symbol_op(&db, "context", &result, symbol, 0);
        print_json(&json);
    }
    Ok(())
}

pub fn cmd_deps(path: &str) -> CmdResult {
    cmd_single_file_op("deps", path, operations::get_deps)
}

pub fn cmd_scope(path: &str, line: u32) -> CmdResult {
    cmd_single_file_op("scope", path, |db, p| operations::get_scope(db, p, line))
}
