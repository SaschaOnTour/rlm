//! MCP read-side query tool handlers: `search`, `overview`, `refs`, `files`.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::query::tree;
use crate::application::symbol::RefsQuery;
use crate::db::Database;
use crate::interface::shared::{
    record_operation, record_symbol_query, AlternativeCost, OperationMeta,
};
use crate::operations;
use crate::output::Formatter;

use super::server::RlmServer;

/// Handle the `search` tool: full-text search across indexed chunks.
// qual:api
pub fn handle_search(
    db: &Database,
    query: &str,
    limit: usize,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match operations::search_chunks(db, query, limit) {
        Ok(result) => {
            let meta = OperationMeta {
                command: "search",
                files_touched: result.file_count,
                alternative: AlternativeCost::AtLeastBody {
                    base: result.tokens.output,
                },
            };
            let response = record_operation(db, &meta, &result);
            Ok(RlmServer::success_text(formatter, response.body))
        }
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `overview` tool: project structure at three detail levels.
// qual:api
pub fn handle_overview(
    db: &Database,
    detail: &str,
    path: Option<&str>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let meta = OperationMeta {
        command: "overview",
        files_touched: 0,
        alternative: AlternativeCost::ScopedFiles {
            prefix: path.map(String::from),
        },
    };

    match detail {
        "minimal" => {
            use crate::application::query::peek;
            match peek::peek(db, path) {
                Ok(result) => {
                    let response = record_operation(db, &meta, &result);
                    Ok(RlmServer::success_text(formatter, response.body))
                }
                Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
            }
        }
        "standard" => match operations::build_map(db, path) {
            Ok(entries) => {
                let response = record_operation(db, &meta, &entries);
                Ok(RlmServer::success_text(formatter, response.body))
            }
            Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
        },
        "tree" => match tree::build_tree(db, path) {
            Ok(nodes) => {
                let response = record_operation(db, &meta, &nodes);
                Ok(RlmServer::success_text(formatter, response.body))
            }
            Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
        },
        other => Ok(RlmServer::error_text(
            formatter,
            format!("unknown detail level: '{other}'. Use 'minimal', 'standard', or 'tree'."),
        )),
    }
}

/// Handle the `refs` tool: find all usages of a symbol with impact analysis.
// qual:api
pub fn handle_refs(
    db: &Database,
    symbol: &str,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match record_symbol_query::<RefsQuery>(db, symbol) {
        Ok(response) => Ok(RlmServer::success_text(formatter, response.body)),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}

/// Handle the `files` tool: list all project files.
// qual:api
pub fn handle_files(
    project_root: &std::path::Path,
    path_prefix: Option<String>,
    skipped_only: bool,
    indexed_only: bool,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let filter = operations::FilesFilter {
        path_prefix,
        skipped_only,
        indexed_only,
    };

    match operations::list_files(project_root, filter) {
        Ok(result) => Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&result),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}
