//! Utility tool handlers for the MCP server.
//!
//! Contains handlers for utility/diagnostic tools: stats, partition, summarize,
//! diff, context, deps, scope, savings, verify, supported.
//!
//! Separated from `tool_handlers.rs` (orient + search + analyze + edit handlers)
//! for SRP compliance.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::symbol::{ContextQuery, ContextWithGraphQuery};
use crate::config::Config;
use crate::db::Database;
use crate::interface::shared::{
    record_operation, record_symbol_query, AlternativeCost, OperationMeta,
};
use crate::operations;
use crate::operations::savings;
use crate::output::Formatter;
use crate::rlm::{partition, summarize};

use super::server::RlmServer;

/// Handle the `stats` tool: get indexing statistics.
// qual:api
pub fn handle_stats(db: &Database, formatter: Formatter) -> Result<CallToolResult, McpError> {
    match operations::get_stats(db) {
        Ok(result) => Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&result),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `partition` tool: split a file into chunks.
// qual:api
pub fn handle_partition(
    db: &Database,
    path: &str,
    strategy_str: &str,
    project_root: &std::path::Path,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let strategy = if strategy_str == "semantic" {
        partition::Strategy::Semantic
    } else if let Some(rest) = strategy_str.strip_prefix("uniform:") {
        match rest.parse::<usize>() {
            Ok(0) => {
                return Ok(RlmServer::error_text(
                    formatter,
                    "uniform chunk size must be >= 1".into(),
                ))
            }
            Ok(n) => partition::Strategy::Uniform(n),
            Err(e) => {
                return Ok(RlmServer::error_text(
                    formatter,
                    format!("invalid chunk size: {e}"),
                ))
            }
        }
    } else if let Some(rest) = strategy_str.strip_prefix("keyword:") {
        partition::Strategy::Keyword(rest.to_string())
    } else {
        return Ok(RlmServer::error_text(
            formatter,
            "strategy must be: semantic, uniform:N, or keyword:PATTERN".into(),
        ));
    };

    match partition::partition_file(db, path, &strategy, project_root) {
        Ok(result) => {
            let meta = OperationMeta {
                command: "partition",
                files_touched: 1,
                alternative: AlternativeCost::SingleFile {
                    path: path.to_string(),
                },
            };
            let response = record_operation(db, &meta, &result);
            Ok(RlmServer::success_text(formatter, response.body))
        }
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `summarize` tool: generate a condensed file summary.
// qual:api
pub fn handle_summarize(
    db: &Database,
    path: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let result = summarize::summarize(db, path);
    match result {
        Ok(val) => {
            let meta = OperationMeta {
                command: "summarize",
                files_touched: 1,
                alternative: AlternativeCost::SingleFile {
                    path: path.to_string(),
                },
            };
            let response = record_operation(db, &meta, &val);
            Ok(RlmServer::success_text(formatter, response.body))
        }
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `diff` tool: compare indexed vs disk version.
// qual:api
pub fn handle_diff(
    db: &Database,
    path: &str,
    symbol: Option<&str>,
    project_root: &std::path::Path,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let meta = OperationMeta {
        command: "diff",
        files_touched: 1,
        alternative: AlternativeCost::SingleFile {
            path: path.to_string(),
        },
    };

    if let Some(sym) = symbol {
        match operations::diff_symbol(db, path, sym, project_root) {
            Ok(result) => {
                let response = record_operation(db, &meta, &result);
                Ok(RlmServer::success_text(formatter, response.body))
            }
            Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
        }
    } else {
        match operations::diff_file(db, path, project_root) {
            Ok(result) => {
                let response = record_operation(db, &meta, &result);
                Ok(RlmServer::success_text(formatter, response.body))
            }
            Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
        }
    }
}

/// Handle the `context` tool: complete understanding of a symbol.
// qual:api
pub fn handle_context(
    db: &Database,
    symbol: &str,
    include_graph: bool,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let result = if include_graph {
        record_symbol_query::<ContextWithGraphQuery>(db, symbol)
    } else {
        record_symbol_query::<ContextQuery>(db, symbol)
    };
    match result {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `deps` tool: file dependency analysis.
// qual:api
pub fn handle_deps(
    db: &Database,
    path: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let result = operations::get_deps(db, path);
    match result {
        Ok(val) => {
            let meta = OperationMeta {
                command: "deps",
                files_touched: 1,
                alternative: AlternativeCost::SingleFile {
                    path: path.to_string(),
                },
            };
            let response = record_operation(db, &meta, &val);
            Ok(RlmServer::success_text(formatter, response.body))
        }
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `scope` tool: symbols visible at a specific line.
// qual:api
pub fn handle_scope(
    db: &Database,
    path: &str,
    line: u32,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match operations::get_scope(db, path, line) {
        Ok(result) => {
            let meta = OperationMeta {
                command: "scope",
                files_touched: 1,
                alternative: AlternativeCost::SingleFile {
                    path: path.to_string(),
                },
            };
            let response = record_operation(db, &meta, &result);
            Ok(RlmServer::success_text(formatter, response.body))
        }
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `savings` tool: token savings report.
// qual:api
pub fn handle_savings(
    db: &Database,
    since: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match savings::get_savings_report(db, since) {
        Ok(report) => Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&report),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `verify` tool: verify index integrity.
// qual:api
pub fn handle_verify(
    config: &Config,
    fix: bool,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let db = match crate::db::Database::open_required(&config.db_path) {
        Ok(db) => db,
        Err(crate::error::RlmError::IndexNotFound) => {
            return Ok(RlmServer::error_text(
                formatter,
                "Index not found. Call the 'index' tool first.".into(),
            ));
        }
        Err(e) => return Err(McpError::internal_error(e.to_string(), None)),
    };

    let report = match operations::verify_index(&db, &config.project_root) {
        Ok(r) => r,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };

    if fix && !report.is_ok() {
        match operations::fix_integrity(&db, &report) {
            Ok(fix_result) => Ok(RlmServer::success_text(
                formatter,
                RlmServer::to_json(&fix_result),
            )),
            Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
        }
    } else {
        Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&report),
        ))
    }
}

/// Handle the `supported` tool: list supported file extensions.
// qual:api
pub fn handle_supported(formatter: Formatter) -> Result<CallToolResult, McpError> {
    Ok(RlmServer::success_text(
        formatter,
        RlmServer::to_json(&operations::list_supported()),
    ))
}
