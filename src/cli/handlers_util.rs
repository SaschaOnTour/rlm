//! CLI handlers for system/utility commands.
//!
//! Code-exploration commands live in `cli::handlers`.
//! Shared helpers live in `cli::helpers`.

use crate::cli::helpers::{get_config, get_db, map_err, should_filter_unknown, CmdResult};
use crate::ingest::code::quality_log;
use crate::operations;
use crate::operations::savings;
use crate::output::Formatter;

pub fn cmd_stats(show_savings: bool, since: Option<&str>, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if show_savings {
        let report = savings::get_savings_report(&db, since).map_err(map_err)?;
        formatter.print(&report);
        return Ok(());
    }

    let result = operations::get_stats(&db).map_err(map_err)?;
    formatter.print(&result);

    // Check for files with quality issues (output to stderr as diagnostic info)
    if let Ok(Some(quality_info)) = operations::get_quality_info(&db) {
        eprintln!("{}", formatter.serialize(&quality_info));
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

/// Clear the quality log and return early (integration: calls only).
fn cmd_quality_clear(log_path: &std::path::Path, formatter: Formatter) -> CmdResult {
    let logger = quality_log::QualityLogger::new(log_path, true);
    logger.clear().map_err(map_err)?;
    formatter.print(&serde_json::json!({"cleared": true}));
    Ok(())
}

/// Display quality issues or summary (integration: calls only).
fn cmd_quality_display(
    issues: Vec<quality_log::QualityIssue>,
    summary: bool,
    formatter: Formatter,
) {
    if summary {
        let stats = quality_log::summarize_issues(&issues);
        formatter.print(&stats);
    } else {
        #[derive(serde::Serialize)]
        struct QualityOutput {
            count: usize,
            issues: Vec<quality_log::QualityIssue>,
        }

        formatter.print(&QualityOutput {
            count: issues.len(),
            issues,
        });
    }
}

pub fn cmd_quality(
    unknown_only: bool,
    all: bool,
    clear: bool,
    summary: bool,
    formatter: Formatter,
) -> CmdResult {
    let config = get_config()?;
    let log_path = config.get_quality_log_path();

    if clear {
        return cmd_quality_clear(&log_path, formatter);
    }

    let mut issues = quality_log::read_quality_log(&log_path).map_err(map_err)?;
    quality_log::annotate_known_issues(&mut issues);

    if should_filter_unknown(unknown_only, all) {
        issues = quality_log::filter_unknown(issues);
    }

    cmd_quality_display(issues, summary, formatter);
    Ok(())
}

pub fn cmd_files(
    path_filter: Option<&str>,
    skipped_only: bool,
    indexed_only: bool,
    formatter: Formatter,
) -> CmdResult {
    let config = get_config()?;
    let filter = operations::FilesFilter {
        path_prefix: path_filter.map(String::from),
        skipped_only,
        indexed_only,
    };
    let result = operations::list_files(&config.project_root, filter).map_err(map_err)?;
    formatter.print(&result);
    Ok(())
}

pub fn cmd_verify(fix: bool, formatter: Formatter) -> CmdResult {
    let config = get_config()?;
    let db = match crate::db::Database::open_required(&config.db_path) {
        Ok(db) => db,
        Err(crate::error::RlmError::IndexNotFound) => {
            return Err(map_err("Index not found. Run 'rlm index' first."));
        }
        Err(e) => return Err(map_err(e.to_string())),
    };
    let report = operations::verify_index(&db, &config.project_root).map_err(map_err)?;

    if fix && !report.is_ok() {
        let fix_result = operations::fix_integrity(&db, &report).map_err(map_err)?;
        formatter.print(&fix_result);
    } else {
        formatter.print(&report);
    }
    Ok(())
}

pub fn cmd_supported(formatter: Formatter) -> CmdResult {
    let result = operations::list_supported();
    formatter.print(&result);
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
    formatter.print(&report);
    Ok(())
}
