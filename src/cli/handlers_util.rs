//! CLI handlers for system/utility commands.
//!
//! Code-exploration commands live in `cli::handlers`.
//! Shared helpers live in `cli::helpers`.

use crate::cli::helpers::{get_config, get_db, map_err, should_filter_unknown, CmdResult};
use crate::ingest::code::quality_log;
use crate::operations;
use crate::operations::savings;
use crate::output;

pub fn cmd_stats(show_savings: bool, since: Option<&str>) -> CmdResult {
    let config = get_config()?;
    let db = get_db(&config)?;

    if show_savings {
        let report = savings::get_savings_report(&db, since).map_err(map_err)?;
        output::print(&report);
        return Ok(());
    }

    let result = operations::get_stats(&db).map_err(map_err)?;
    output::print(&result);

    // Check for files with quality issues (output to stderr as diagnostic info)
    if let Ok(Some(quality_info)) = operations::get_quality_info(&db) {
        eprintln!("{}", output::serialize(&quality_info));
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
fn cmd_quality_clear(log_path: &std::path::Path) -> CmdResult {
    let logger = quality_log::QualityLogger::new(log_path, true);
    logger.clear().map_err(map_err)?;
    output::print(&serde_json::json!({"cleared": true}));
    Ok(())
}

/// Display quality issues or summary (integration: calls only).
fn cmd_quality_display(issues: Vec<quality_log::QualityIssue>, summary: bool) {
    if summary {
        let stats = quality_log::summarize_issues(&issues);
        output::print(&stats);
    } else {
        #[derive(serde::Serialize)]
        struct QualityOutput {
            count: usize,
            issues: Vec<quality_log::QualityIssue>,
        }

        output::print(&QualityOutput {
            count: issues.len(),
            issues,
        });
    }
}

pub fn cmd_quality(unknown_only: bool, all: bool, clear: bool, summary: bool) -> CmdResult {
    let config = get_config()?;
    let log_path = config.get_quality_log_path();

    if clear {
        return cmd_quality_clear(&log_path);
    }

    let mut issues = quality_log::read_quality_log(&log_path).map_err(map_err)?;
    quality_log::annotate_known_issues(&mut issues);

    if should_filter_unknown(unknown_only, all) {
        issues = quality_log::filter_unknown(issues);
    }

    cmd_quality_display(issues, summary);
    Ok(())
}

pub fn cmd_files(path_filter: Option<&str>, skipped_only: bool, indexed_only: bool) -> CmdResult {
    let config = get_config()?;
    let filter = operations::FilesFilter {
        path_prefix: path_filter.map(String::from),
        skipped_only,
        indexed_only,
    };
    let result = operations::list_files(&config.project_root, filter).map_err(map_err)?;
    output::print(&result);
    Ok(())
}

pub fn cmd_verify(fix: bool) -> CmdResult {
    let config = get_config()?;

    if !config.index_exists() {
        return Err(map_err("Index not found. Run 'rlm index' first."));
    }

    let db = crate::db::Database::open(&config.db_path).map_err(map_err)?;
    let report = operations::verify_index(&db, &config.project_root).map_err(map_err)?;

    if fix && !report.is_ok() {
        let fix_result = operations::fix_integrity(&db, &report).map_err(map_err)?;
        output::print(&fix_result);
    } else {
        output::print(&report);
    }
    Ok(())
}

pub fn cmd_supported() -> CmdResult {
    let result = operations::list_supported();
    output::print(&result);
    Ok(())
}
