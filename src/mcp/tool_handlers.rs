//! Core tool handlers for the MCP server (orient + search + analyze + edit).
//!
//! Contains: handle_index, handle_search, handle_read, handle_overview,
//! handle_refs, handle_replace, handle_insert, handle_files.
//!
//! Utility handlers live in `tool_handlers_util`.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::config::Config;
use crate::db::Database;
use crate::edit::inserter::InsertPosition;
use crate::edit::syntax_guard::SyntaxGuard;
use crate::edit::{inserter, replacer};
use crate::indexer;
use crate::models::token_estimate::estimate_json_tokens;
use crate::operations;
use crate::operations::savings;
use crate::search::tree;

use super::server::RlmServer;
use super::tools::ReadParams;

/// Build the JSON response for a successful write operation, auto-reindexing the file.
///
/// Attempts to reindex the modified file so refs/context/search stay up-to-date.
/// Returns `{"ok":true,"reindexed":true}` on success, or `{"ok":true,"reindexed":false,"hint":"..."}` if reindex fails.
fn write_result_with_reindex(
    db: &Database,
    project_root: &std::path::Path,
    rel_path: &str,
    symbol: Option<&str>,
) -> String {
    let config = crate::config::Config::new(project_root);
    indexer::reindex_with_result(db, &config, rel_path, symbol)
}

/// Resolve the index config, validating that any custom path is within project_root.
fn resolve_index_config(
    path: Option<&str>,
    project_root: &std::path::Path,
) -> Result<Config, McpError> {
    match path {
        Some(p) => {
            let abs = std::path::Path::new(p);
            let canonical = abs
                .canonicalize()
                .map_err(|e| McpError::invalid_request(e.to_string(), None))?;
            let root = project_root
                .canonicalize()
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            if !canonical.starts_with(&root) {
                return Err(McpError::invalid_request(
                    "index path must be within the project root",
                    None,
                ));
            }
            Ok(Config::new(&canonical))
        }
        None => Ok(Config::new(project_root)),
    }
}

/// Handle the `index` tool: scan and index the codebase.
// qual:api
pub fn handle_index(
    path: Option<&str>,
    project_root: &std::path::Path,
) -> Result<CallToolResult, McpError> {
    let config = resolve_index_config(path, project_root)?;

    if let Err(e) = config.ensure_rlm_dir() {
        return Ok(RlmServer::error_text(e.to_string()));
    }

    match indexer::run_index(&config) {
        Ok(result) => {
            let output: operations::IndexOutput = result.into();
            Ok(RlmServer::success_text(RlmServer::to_json(&output)))
        }
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `search` tool: full-text search across indexed chunks.
// qual:api
pub fn handle_search(db: &Database, query: &str, limit: usize) -> Result<CallToolResult, McpError> {
    match operations::search_chunks(db, query, limit) {
        Ok(result) => {
            let json = RlmServer::to_json(&result);
            let out_tokens = estimate_json_tokens(json.len());
            let alt_tokens = result.tokens.output.max(out_tokens);
            savings::record(
                db,
                "search",
                out_tokens,
                alt_tokens,
                result.results.len() as u64,
            );
            Ok(RlmServer::success_text(json))
        }
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `read` tool: read a specific symbol or markdown section.
// qual:api
pub fn handle_read(db: &Database, params: &ReadParams) -> Result<CallToolResult, McpError> {
    match (&params.symbol, &params.section) {
        (Some(_), _) => handle_read_symbol(db, params),
        (_, Some(_)) => handle_read_section(db, params),
        _ => Ok(RlmServer::error_text(
            "read requires 'symbol' or 'section'. Use Claude Code's Read for full files or line ranges.".into(),
        )),
    }
}

/// Filter chunks to those belonging to a specific file path (operation: logic only).
fn filter_chunks_by_path<'a>(
    db: &Database,
    chunks: &'a [crate::models::chunk::Chunk],
    path: &str,
) -> Vec<&'a crate::models::chunk::Chunk> {
    // Single O(1) lookup instead of loading all files and scanning O(files × chunks)
    let file_id = match db.get_file_by_path(path) {
        Ok(Some(f)) => f.id,
        _ => return Vec::new(),
    };
    chunks.iter().filter(|c| c.file_id == file_id).collect()
}

/// Resolve which chunks to return and build the result (integration: calls only).
// qual:allow(iosp) reason: "MCP handler with inherent error matching and delegation"
fn handle_read_symbol(db: &Database, params: &ReadParams) -> Result<CallToolResult, McpError> {
    let sym = params.symbol.as_deref().unwrap_or_default();
    let chunks = match db.get_chunks_by_ident(sym) {
        Ok(c) => c,
        Err(e) => return Ok(RlmServer::error_text(e.to_string())),
    };

    if chunks.is_empty() {
        return Ok(RlmServer::error_text(format!(
            "Symbol not found: {sym}. Use 'search' to find similar symbols, or check the 'path' parameter."
        )));
    }

    let file_chunks = filter_chunks_by_path(db, &chunks, &params.path);

    if file_chunks.is_empty() {
        RlmServer::read_symbol_result(db, params, &chunks)
    } else {
        RlmServer::read_symbol_result(db, params, &file_chunks)
    }
}

fn handle_read_section(db: &Database, params: &ReadParams) -> Result<CallToolResult, McpError> {
    let heading = params.section.as_deref().unwrap_or_default();
    match db.get_file_by_path(&params.path) {
        Ok(Some(file)) => match db.get_chunks_for_file(file.id) {
            Ok(chunks) => match chunks.iter().find(|c| c.ident == *heading) {
                Some(c) => {
                    let json = savings::record_file_op(db, "read_section", c, &params.path);
                    Ok(RlmServer::success_text(json))
                }
                None => {
                    let available: Vec<&str> = chunks.iter().map(|c| c.ident.as_str()).collect();
                    let hint = if available.is_empty() {
                        format!("Section not found: {heading}. File has no sections.")
                    } else {
                        format!(
                            "Section not found: {heading}. Available: {}",
                            available.join(", ")
                        )
                    };
                    Ok(RlmServer::error_text(hint))
                }
            },
            Err(e) => Ok(RlmServer::error_text(e.to_string())),
        },
        Ok(None) => Ok(RlmServer::error_text(format!(
            "File not found: {}. Run 'index' to update, or check 'files' for available paths.",
            params.path
        ))),
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `overview` tool: project structure at three detail levels.
// qual:api
pub fn handle_overview(
    db: &Database,
    detail: &str,
    path: Option<&str>,
) -> Result<CallToolResult, McpError> {
    match detail {
        "minimal" => {
            use crate::rlm::peek;
            match peek::peek(db, path) {
                Ok(result) => {
                    let json = savings::record_scoped_op(db, "overview", &result, path);
                    Ok(RlmServer::success_text(json))
                }
                Err(e) => Ok(RlmServer::error_text(e.to_string())),
            }
        }
        "standard" => match operations::build_map(db, path) {
            Ok(entries) => {
                let json = savings::record_scoped_op(db, "overview", &entries, path);
                Ok(RlmServer::success_text(json))
            }
            Err(e) => Ok(RlmServer::error_text(e.to_string())),
        },
        "tree" => match tree::build_tree(db, path) {
            Ok(nodes) => {
                let json = savings::record_scoped_op(db, "overview", &nodes, path);
                Ok(RlmServer::success_text(json))
            }
            Err(e) => Ok(RlmServer::error_text(e.to_string())),
        },
        other => Ok(RlmServer::error_text(format!(
            "unknown detail level: '{other}'. Use 'minimal', 'standard', or 'tree'."
        ))),
    }
}

/// Handle the `refs` tool: find all usages of a symbol with impact analysis.
// qual:api
pub fn handle_refs(db: &Database, symbol: &str) -> Result<CallToolResult, McpError> {
    match operations::analyze_impact(db, symbol) {
        Ok(result) => {
            let files_touched = result.count as u64;
            let json = savings::record_symbol_op(db, "refs", &result, symbol, files_touched);
            Ok(RlmServer::success_text(json))
        }
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `replace` tool: preview or apply a replacement.
// qual:api
pub fn handle_replace(
    db: &Database,
    params: &super::tools::ReplaceParams,
    project_root: &std::path::Path,
) -> Result<CallToolResult, McpError> {
    if params.preview.unwrap_or(false) {
        match replacer::preview_replace(db, &params.path, &params.symbol, &params.code) {
            Ok(diff) => Ok(RlmServer::success_text(RlmServer::to_json(&diff))),
            Err(e) => Ok(RlmServer::error_text(e.to_string())),
        }
    } else {
        match replacer::replace_symbol(db, &params.path, &params.symbol, &params.code, project_root)
        {
            Ok(outcome) => {
                let result_json =
                    write_result_with_reindex(db, project_root, &params.path, Some(&params.symbol));
                if let Ok(entry) = savings::alternative_replace_entry(
                    db,
                    &params.path,
                    outcome.old_code_len,
                    params.code.len(),
                    result_json.len(),
                ) {
                    savings::record_v2(db, &entry);
                }
                Ok(RlmServer::success_text(result_json))
            }
            Err(e) => Ok(RlmServer::error_text(e.to_string())),
        }
    }
}

/// Handle the `insert` tool: insert code at a specified position.
// qual:api
pub fn handle_insert(
    db: Option<&Database>,
    path: &str,
    position: &InsertPosition,
    code: &str,
    project_root: &std::path::Path,
) -> Result<CallToolResult, McpError> {
    let guard = SyntaxGuard::new();
    match inserter::insert_code(project_root, path, position, code, &guard) {
        Ok(_) => match db {
            Some(db) => {
                let result_json = write_result_with_reindex(db, project_root, path, None);
                if let Ok(entry) =
                    savings::alternative_insert_entry(db, path, code.len(), result_json.len())
                {
                    savings::record_v2(db, &entry);
                }
                Ok(RlmServer::success_text(result_json))
            }
            None => Ok(RlmServer::success_text(
                serde_json::json!({"ok": true, "reindexed": false, "hint": "no index; call 'index' to enable auto-reindex"})
                    .to_string(),
            )),
        },
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

/// Handle the `files` tool: list all project files.
// qual:api
pub fn handle_files(
    project_root: &std::path::Path,
    path_prefix: Option<String>,
    skipped_only: bool,
    indexed_only: bool,
) -> Result<CallToolResult, McpError> {
    let filter = operations::FilesFilter {
        path_prefix,
        skipped_only,
        indexed_only,
    };

    match operations::list_files(project_root, filter) {
        Ok(result) => Ok(RlmServer::success_text(RlmServer::to_json(&result))),
        Err(e) => Ok(RlmServer::error_text(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::inserter::InsertPosition;

    #[test]
    fn insert_with_relative_path_resolves_to_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {}\n").unwrap();

        let config = crate::config::Config::new(dir.path());
        config.ensure_rlm_dir().unwrap();
        let db = Database::open(&config.db_path).unwrap();

        let result = handle_insert(
            Some(&db),
            "test.rs",
            &InsertPosition::Top,
            "// header\n",
            dir.path(),
        );
        assert!(
            result.is_ok(),
            "insert should succeed with relative path + project_root"
        );

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(
            content.starts_with("// header"),
            "file should have inserted content at top"
        );
    }

    #[test]
    fn insert_with_nonexistent_relative_path_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config::new(dir.path());
        config.ensure_rlm_dir().unwrap();
        let db = Database::open(&config.db_path).unwrap();

        let result = handle_insert(
            Some(&db),
            "nonexistent.rs",
            &InsertPosition::Top,
            "// hi\n",
            dir.path(),
        );
        let call_result = result.unwrap();
        assert_eq!(call_result.is_error, Some(true));
    }
}
