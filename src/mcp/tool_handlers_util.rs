//! Utility tool handlers for the MCP server.
//!
//! Contains handlers for utility/diagnostic tools: stats, partition, summarize,
//! diff, context, deps, scope, savings, verify, supported.
//!
//! Separated from `tool_handlers.rs` (orient + search + analyze + edit handlers)
//! for SRP compliance.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use serde::Serialize;

use crate::config::Config;
use crate::db::Database;
use crate::operations;
use crate::operations::savings;
use crate::rlm::{partition, summarize};

use super::server::RlmServer;

/// Handle the `stats` tool: get indexing statistics.
// qual:api
pub fn handle_stats(db: &Database) -> Result<CallToolResult, McpError> {
    match operations::get_stats(db) {
        Ok(result) => Ok(RlmServer::success_text(RlmServer::to_json(&result))),
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `partition` tool: split a file into chunks.
// qual:api
pub fn handle_partition(
    db: &Database,
    path: &str,
    strategy_str: &str,
    project_root: &std::path::Path,
) -> Result<CallToolResult, McpError> {
    let strategy = if strategy_str == "semantic" {
        partition::Strategy::Semantic
    } else if let Some(rest) = strategy_str.strip_prefix("uniform:") {
        match rest.parse::<usize>() {
            Ok(0) => {
                return Ok(RlmServer::error_text(
                    "uniform chunk size must be >= 1".into(),
                ))
            }
            Ok(n) => partition::Strategy::Uniform(n),
            Err(e) => return Ok(RlmServer::error_text(format!("invalid chunk size: {e}"))),
        }
    } else if let Some(rest) = strategy_str.strip_prefix("keyword:") {
        partition::Strategy::Keyword(rest.to_string())
    } else {
        return Ok(RlmServer::error_text(
            "strategy must be: semantic, uniform:N, or keyword:PATTERN".into(),
        ));
    };

    match partition::partition_file(db, path, &strategy, project_root) {
        Ok(result) => {
            let json = savings::record_file_op(db, "partition", &result, path);
            Ok(RlmServer::success_text(json))
        }
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `summarize` tool: generate a condensed file summary.
// qual:api
pub fn handle_summarize(db: &Database, path: &str) -> Result<CallToolResult, McpError> {
    let result = summarize::summarize(db, path);
    match result {
        Ok(val) => {
            let json = savings::record_file_op(db, "summarize", &val, path);
            Ok(RlmServer::success_text(json))
        }
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `diff` tool: compare indexed vs disk version.
// qual:api
pub fn handle_diff(
    db: &Database,
    path: &str,
    symbol: Option<&str>,
    project_root: &std::path::Path,
) -> Result<CallToolResult, McpError> {
    if let Some(sym) = symbol {
        match operations::diff_symbol(db, path, sym, project_root) {
            Ok(result) => {
                let json = savings::record_file_op(db, "diff", &result, path);
                Ok(RlmServer::success_text(json))
            }
            Err(e) => Ok(RlmServer::error_text(e.to_string())),
        }
    } else {
        match operations::diff_file(db, path, project_root) {
            Ok(result) => {
                let json = savings::record_file_op(db, "diff", &result, path);
                Ok(RlmServer::success_text(json))
            }
            Err(e) => Ok(RlmServer::error_text(e.to_string())),
        }
    }
}

/// Handle the `context` tool: complete understanding of a symbol.
// qual:api
pub fn handle_context(
    db: &Database,
    symbol: &str,
    include_graph: bool,
) -> Result<CallToolResult, McpError> {
    match operations::build_context(db, symbol) {
        Ok(ctx_result) => {
            if include_graph {
                match operations::build_callgraph(db, symbol) {
                    Ok(graph) => {
                        #[derive(Serialize)]
                        struct ContextWithGraph<'a> {
                            context: &'a operations::ContextResult,
                            callgraph: &'a operations::CallgraphResult,
                        }
                        let combined = ContextWithGraph {
                            context: &ctx_result,
                            callgraph: &graph,
                        };
                        let json = savings::record_symbol_op(db, "context", &combined, symbol, 0);
                        Ok(RlmServer::success_text(json))
                    }
                    Err(e) => Ok(RlmServer::error_text(e.to_string())),
                }
            } else {
                let json = savings::record_symbol_op(db, "context", &ctx_result, symbol, 0);
                Ok(RlmServer::success_text(json))
            }
        }
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `deps` tool: file dependency analysis.
// qual:api
pub fn handle_deps(db: &Database, path: &str) -> Result<CallToolResult, McpError> {
    let result = operations::get_deps(db, path);
    match result {
        Ok(val) => {
            let json = savings::record_file_op(db, "deps", &val, path);
            Ok(RlmServer::success_text(json))
        }
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `scope` tool: symbols visible at a specific line.
// qual:api
pub fn handle_scope(db: &Database, path: &str, line: u32) -> Result<CallToolResult, McpError> {
    match operations::get_scope(db, path, line) {
        Ok(result) => {
            let json = savings::record_file_op(db, "scope", &result, path);
            Ok(RlmServer::success_text(json))
        }
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `savings` tool: token savings report.
// qual:api
pub fn handle_savings(db: &Database, since: Option<&str>) -> Result<CallToolResult, McpError> {
    match savings::get_savings_report(db, since) {
        Ok(report) => Ok(RlmServer::success_text(RlmServer::to_json(&report))),
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `verify` tool: verify index integrity.
// qual:api
pub fn handle_verify(config: &Config, fix: bool) -> Result<CallToolResult, McpError> {
    if !config.index_exists() {
        return Ok(RlmServer::error_text(
            "Index not found. Call the 'index' tool first.".into(),
        ));
    }

    let db = crate::db::Database::open(&config.db_path)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    let report = match operations::verify_index(&db, &config.project_root) {
        Ok(r) => r,
        Err(e) => return Ok(RlmServer::error_text(e.to_string())),
    };

    if fix && !report.is_ok() {
        match operations::fix_integrity(&db, &report) {
            Ok(fix_result) => Ok(RlmServer::success_text(RlmServer::to_json(&fix_result))),
            Err(e) => Ok(RlmServer::error_text(e.to_string())),
        }
    } else {
        Ok(RlmServer::success_text(RlmServer::to_json(&report)))
    }
}

/// Handle the `supported` tool: list supported file extensions.
// qual:api
pub fn handle_supported() -> Result<CallToolResult, McpError> {
    Ok(RlmServer::success_text(RlmServer::to_json(
        &operations::list_supported(),
    )))
}
