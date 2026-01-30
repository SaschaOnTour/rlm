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
use rlm::cli::output;
use rlm::config::Config;
use rlm::db::Database;
use rlm::edit::syntax_guard::SyntaxGuard;
use rlm::edit::{inserter, replacer};
use rlm::indexer;
use rlm::ingest::code::quality_log;
use rlm::operations::{self, parse_position};
use rlm::rlm::{batch, grep, partition, peek, summarize};
use rlm::search::tree;

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("{}", output::format_error(&e));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::fmt::Display>> {
    match cli.command {
        Command::Index { path } => cmd_index(&path),
        Command::Search { query, limit } => cmd_search(&query, limit),
        Command::Read {
            path,
            symbol,
            section,
            lines,
        } => cmd_read(
            &path,
            symbol.as_deref(),
            section.as_deref(),
            lines.as_deref(),
        ),
        Command::Tree => cmd_tree(),
        Command::Refs { symbol } => cmd_refs(&symbol),
        Command::Signature { symbol } => cmd_signature(&symbol),
        Command::Replace {
            path,
            symbol,
            code,
            preview,
        } => cmd_replace(&path, &symbol, &code, preview),
        Command::Insert {
            path,
            code,
            position,
        } => cmd_insert(&path, &code, &position),
        Command::Stats => cmd_stats(),
        Command::Peek { path } => cmd_peek(path.as_deref()),
        Command::Grep {
            pattern,
            context,
            path,
        } => cmd_grep(&pattern, context, path.as_deref()),
        Command::Partition { path, strategy } => cmd_partition(&path, &strategy),
        Command::Summarize { path } => cmd_summarize(&path),
        Command::Batch { query, limit } => cmd_batch(&query, limit),
        Command::Diff { path, symbol } => cmd_diff(&path, symbol.as_deref()),
        Command::Map { path } => cmd_map(path.as_deref()),
        Command::Callgraph { symbol } => cmd_callgraph(&symbol),
        Command::Impact { symbol } => cmd_impact(&symbol),
        Command::Context { symbol } => cmd_context(&symbol),
        Command::Deps { path } => cmd_deps(&path),
        Command::Scope { path, line } => cmd_scope(&path, line),
        Command::Type { symbol } => cmd_type(&symbol),
        Command::Patterns { query } => cmd_patterns(&query),
        Command::Mcp => cmd_mcp(),
        Command::Quality {
            unknown_only,
            all,
            clear,
            summary,
        } => cmd_quality(unknown_only, all, clear, summary),
        Command::Files {
            path,
            skipped_only,
            indexed_only,
        } => cmd_files(path.as_deref(), skipped_only, indexed_only),
        Command::Verify { fix } => cmd_verify(fix),
        Command::Supported => cmd_supported(),
    }
}

type CmdResult = Result<(), Box<dyn std::fmt::Display>>;

fn map_err(e: impl std::fmt::Display + 'static) -> Box<dyn std::fmt::Display> {
    Box::new(e.to_string())
}

fn get_config() -> Result<Config, Box<dyn std::fmt::Display>> {
    Config::from_cwd().map_err(map_err)
}

fn get_db(config: &Config) -> Result<Database, Box<dyn std::fmt::Display>> {
    indexer::ensure_index(config).map_err(map_err)
}

fn cmd_index(path: &str) -> CmdResult {
    let config = if path == "." {
        get_config()?
    } else {
        Config::new(path)
    };

    let result = indexer::run_index(&config).map_err(map_err)?;
    let output: operations::IndexOutput = result.into();
    println!("{}", output::format_json(&output));
    Ok(())
}

fn cmd_search(query: &str, limit: usize) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::search_chunks(&db, query, limit).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_read(
    path: &str,
    symbol: Option<&str>,
    section: Option<&str>,
    lines: Option<&str>,
) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if let Some(sym) = symbol {
        // Read specific symbol
        let chunks = db.get_chunks_by_ident(sym).map_err(map_err)?;
        let file_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| {
                db.get_all_files()
                    .ok()
                    .is_some_and(|files| files.iter().any(|f| f.id == c.file_id && f.path == path))
            })
            .collect();

        if file_chunks.is_empty() {
            // Try all chunks with that name
            if chunks.is_empty() {
                return Err(map_err(format!("symbol not found: {sym}")));
            }
            println!("{}", output::format_json(&chunks));
        } else {
            println!("{}", output::format_json(&file_chunks));
        }
    } else if let Some(heading) = section {
        // Read specific markdown section
        let file = db.get_file_by_path(path).map_err(map_err)?;
        let file = file.ok_or_else(|| map_err(format!("file not found: {path}")))?;
        let chunks = db.get_chunks_for_file(file.id).map_err(map_err)?;
        let section_chunk = chunks.iter().find(|c| c.ident == heading);
        match section_chunk {
            Some(c) => println!("{}", output::format_json(c)),
            None => return Err(map_err(format!("section not found: {heading}"))),
        }
    } else if let Some(line_range) = lines {
        // Read line range
        let full_path = config.project_root.join(path);
        let source = std::fs::read_to_string(&full_path).map_err(map_err)?;
        let all_lines: Vec<&str> = source.lines().collect();

        let parts: Vec<&str> = line_range.split('-').collect();
        if parts.len() != 2 {
            return Err(map_err("line range must be in format START-END"));
        }
        let start: usize = parts[0].parse().map_err(map_err)?;
        let end: usize = parts[1].parse().map_err(map_err)?;
        let start = start.saturating_sub(1);
        let end = end.min(all_lines.len());
        let content = all_lines[start..end].join("\n");
        println!("{}", output::format_with_tokens(content));
    } else {
        // Read full file
        let full_path = config.project_root.join(path);
        let content = std::fs::read_to_string(&full_path).map_err(map_err)?;
        println!("{}", output::format_with_tokens(content));
    }
    Ok(())
}

fn cmd_tree() -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let nodes = tree::build_tree(&db).map_err(map_err)?;
    println!("{}", output::format_with_tokens(nodes));
    Ok(())
}

fn cmd_refs(symbol: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::get_refs(&db, symbol).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_signature(symbol: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::get_signature(&db, symbol).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_replace(path: &str, symbol: &str, code: &str, preview: bool) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if preview {
        let diff = replacer::preview_replace(&db, path, symbol, code).map_err(map_err)?;
        #[derive(serde::Serialize)]
        struct DiffOutput {
            file: String,
            symbol: String,
            old_lines: (u32, u32),
            old_code: String,
            new_code: String,
        }
        println!(
            "{}",
            output::format_json(&DiffOutput {
                file: diff.file,
                symbol: diff.symbol,
                old_lines: (diff.start_line, diff.end_line),
                old_code: diff.old_code,
                new_code: diff.new_code,
            })
        );
    } else {
        let guard = SyntaxGuard::new();
        replacer::replace_symbol(&db, path, symbol, code, &guard).map_err(map_err)?;
        println!("{{\"ok\":true}}");
    }
    Ok(())
}

fn cmd_insert(path: &str, code: &str, position: &str) -> CmdResult {
    let pos = parse_position(position).map_err(map_err)?;
    let guard = SyntaxGuard::new();
    inserter::insert_code(path, &pos, code, &guard).map_err(map_err)?;
    println!("{{\"ok\":true}}");
    Ok(())
}

fn cmd_stats() -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    let result = operations::get_stats(&db).map_err(map_err)?;
    println!("{}", output::format_json(&result));

    // Check for files with quality issues (output to stderr as diagnostic info)
    if let Ok(Some(quality_info)) = operations::get_quality_info(&db) {
        eprintln!("{}", output::format_json(&quality_info));
    }

    Ok(())
}

fn cmd_peek(path: Option<&str>) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = peek::peek(&db, path).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_grep(pattern: &str, context: usize, path: Option<&str>) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = grep::grep(&db, pattern, context, path, &config.project_root).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_partition(path: &str, strategy_str: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let strategy = parse_strategy(strategy_str)?;
    let result =
        partition::partition_file(&db, path, &strategy, &config.project_root).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn parse_strategy(s: &str) -> Result<partition::Strategy, Box<dyn std::fmt::Display>> {
    if s == "semantic" {
        Ok(partition::Strategy::Semantic)
    } else if let Some(rest) = s.strip_prefix("uniform:") {
        let n: usize = rest.parse().map_err(map_err)?;
        Ok(partition::Strategy::Uniform(n))
    } else if let Some(rest) = s.strip_prefix("keyword:") {
        Ok(partition::Strategy::Keyword(rest.to_string()))
    } else {
        Err(map_err(
            "strategy must be: semantic, uniform:N, or keyword:PATTERN",
        ))
    }
}

fn cmd_summarize(path: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = summarize::summarize(&db, path).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_batch(query: &str, limit: usize) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = batch::batch_search(&db, query, limit).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_diff(path: &str, symbol: Option<&str>) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if let Some(sym) = symbol {
        let result =
            operations::diff_symbol(&db, path, sym, &config.project_root).map_err(map_err)?;
        println!("{}", output::format_json(&result));
    } else {
        let result = operations::diff_file(&db, path, &config.project_root).map_err(map_err)?;
        println!("{}", output::format_json(&result));
    }
    Ok(())
}

fn cmd_map(path: Option<&str>) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let entries = operations::build_map(&db, path).map_err(map_err)?;
    println!("{}", output::format_with_tokens(entries));
    Ok(())
}

fn cmd_callgraph(symbol: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::build_callgraph(&db, symbol).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_impact(symbol: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::analyze_impact(&db, symbol).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_context(symbol: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::build_context(&db, symbol).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_deps(path: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::get_deps(&db, path).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_scope(path: &str, line: u32) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::get_scope(&db, path, line).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_type(symbol: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::get_type_info(&db, symbol).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_patterns(query: &str) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;
    let result = operations::find_patterns(&db, query).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_mcp() -> CmdResult {
    let rt = tokio::runtime::Runtime::new().map_err(map_err)?;
    rt.block_on(async {
        rlm::mcp::server::start_mcp_server()
            .await
            .map_err(map_err)
    })
}

fn cmd_quality(unknown_only: bool, all: bool, clear: bool, summary: bool) -> CmdResult {
    let config = get_config()?;
    let log_path = config.get_quality_log_path();

    if clear {
        let logger = quality_log::QualityLogger::new(&log_path, true);
        logger.clear().map_err(map_err)?;
        println!("{{\"cleared\":true}}");
        return Ok(());
    }

    let mut issues = quality_log::read_quality_log(&log_path).map_err(map_err)?;

    // Annotate known issues
    quality_log::annotate_known_issues(&mut issues);

    // Filter if requested
    if unknown_only {
        issues = quality_log::filter_unknown(issues);
    } else if !all {
        // Default: show only unknown issues
        issues = quality_log::filter_unknown(issues);
    }

    if summary {
        let stats = quality_log::summarize_issues(&issues);
        println!("{}", output::format_json(&stats));
    } else {
        #[derive(serde::Serialize)]
        struct QualityOutput {
            count: usize,
            issues: Vec<quality_log::QualityIssue>,
        }

        println!(
            "{}",
            output::format_json(&QualityOutput {
                count: issues.len(),
                issues,
            })
        );
    }
    Ok(())
}

fn cmd_files(path_filter: Option<&str>, skipped_only: bool, indexed_only: bool) -> CmdResult {
    let config = get_config()?;
    let filter = operations::FilesFilter {
        path_prefix: path_filter.map(String::from),
        skipped_only,
        indexed_only,
    };
    let result = operations::list_files(&config.project_root, filter).map_err(map_err)?;
    println!("{}", output::format_json(&result));
    Ok(())
}

fn cmd_verify(fix: bool) -> CmdResult {
    let config = get_config()?;

    if !config.index_exists() {
        return Err(map_err("Index not found. Run 'rlm index' first."));
    }

    let db = rlm::db::Database::open(&config.db_path).map_err(map_err)?;
    let report = operations::verify_index(&db, &config.project_root).map_err(map_err)?;

    if fix && !report.is_ok() {
        let fix_result = operations::fix_integrity(&db, &report).map_err(map_err)?;
        println!("{}", output::format_json(&fix_result));
    } else {
        println!("{}", output::format_json(&report));
    }
    Ok(())
}

fn cmd_supported() -> CmdResult {
    let result = operations::list_supported();
    println!("{}", output::format_json(&result));
    Ok(())
}
