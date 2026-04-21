//! MCP write-side tool handlers: `replace` and `insert`.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::edit::inserter::InsertPosition;
use crate::application::edit::validator::SyntaxGuard;
use crate::application::edit::{inserter, replacer};
use crate::application::index as indexer;
use crate::db::Database;
use crate::operations::savings;
use crate::output::Formatter;

use super::server::RlmServer;

/// Build the JSON response for a successful write operation, auto-reindexing the file.
///
/// Attempts to reindex the modified file so refs/context/search stay up-to-date.
/// Returns `{"ok":true,"reindexed":true}` on success, or `{"ok":true,"reindexed":false,"hint":"..."}` if reindex fails.
fn write_result_with_reindex(
    db: &Database,
    project_root: &std::path::Path,
    rel_path: &str,
    source: indexer::PreviewSource<'_>,
) -> String {
    let config = crate::config::Config::new(project_root);
    indexer::reindex_with_result(db, &config, rel_path, source)
}

/// Handle the `replace` tool: preview or apply a replacement.
// qual:api
pub fn handle_replace(
    db: &Database,
    params: &super::tools::ReplaceParams,
    project_root: &std::path::Path,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    if params.preview.unwrap_or(false) {
        match replacer::preview_replace(db, &params.path, &params.symbol, &params.code) {
            Ok(diff) => Ok(RlmServer::success_text(
                formatter,
                RlmServer::to_json(&diff),
            )),
            Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
        }
    } else {
        match replacer::replace_symbol(db, &params.path, &params.symbol, &params.code, project_root)
        {
            Ok(outcome) => {
                let result_json = write_result_with_reindex(
                    db,
                    project_root,
                    &params.path,
                    indexer::PreviewSource::Symbol(&params.symbol),
                );
                if let Ok(entry) = savings::alternative_replace_entry(
                    db,
                    &params.path,
                    outcome.old_code_len,
                    params.code.len(),
                    result_json.len(),
                ) {
                    savings::record_v2(db, &entry);
                }
                Ok(RlmServer::success_text(formatter, result_json))
            }
            Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
        }
    }
}

/// Grouped inputs to `handle_insert` so the handler stays below the
/// SRP_PARAMS ceiling of 5 parameters.
pub struct InsertInput<'a> {
    pub path: &'a str,
    pub position: &'a InsertPosition,
    pub code: &'a str,
}

/// Handle the `insert` tool: insert code at a specified position.
// qual:api
pub fn handle_insert(
    db: Option<&Database>,
    input: &InsertInput<'_>,
    project_root: &std::path::Path,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let guard = SyntaxGuard::new();
    match inserter::insert_code(project_root, input.path, input.position, input.code, &guard) {
        Ok(_) => match db {
            Some(db) => {
                let result_json = write_result_with_reindex(
                    db,
                    project_root,
                    input.path,
                    input.position.preview_source(),
                );
                if let Ok(entry) = savings::alternative_insert_entry(
                    db,
                    input.path,
                    input.code.len(),
                    result_json.len(),
                ) {
                    savings::record_v2(db, &entry);
                }
                Ok(RlmServer::success_text(formatter, result_json))
            }
            None => Ok(RlmServer::success_text(
                formatter,
                serde_json::json!({"ok": true, "reindexed": false, "hint": "no index; call 'index' to enable auto-reindex"})
                    .to_string(),
            )),
        },
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}
