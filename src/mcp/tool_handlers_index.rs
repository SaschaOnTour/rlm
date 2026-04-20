//! MCP `index` tool handler (scan + write to `.rlm/index.db`).

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::index as indexer;
use crate::config::Config;
use crate::operations;
use crate::output::Formatter;

use super::server::RlmServer;

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
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    handle_index_with_progress(path, project_root, None, formatter)
}

/// Handle index with optional progress callback (used by MCP async handler).
// qual:api
pub fn handle_index_with_progress(
    path: Option<&str>,
    project_root: &std::path::Path,
    progress: Option<&indexer::ProgressCallback>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let config = resolve_index_config(path, project_root)?;

    if let Err(e) = config.ensure_rlm_dir() {
        return Ok(RlmServer::error_text(formatter, e.to_string()));
    }

    match indexer::run_index(&config, progress) {
        Ok(result) => {
            let output: operations::IndexOutput = result.into();
            Ok(RlmServer::success_text(
                formatter,
                RlmServer::to_json(&output),
            ))
        }
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}
