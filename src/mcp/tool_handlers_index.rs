//! MCP `index` tool handler (scan + write to `.rlm/index.db`).
//!
//! Index is the one operation that does NOT require an open session —
//! indexing _is_ the act of creating the index. The handler does
//! sandbox the caller-supplied path (must be within the MCP server's
//! project root) and then delegates to
//! [`RlmSession::index_project`](crate::application::session::RlmSession::index_project).

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::session::{ProgressCallback, RlmSession};
use crate::output::Formatter;

use super::server::RlmServer;

/// Canonicalise and validate that `path` is within `project_root`.
/// The MCP server takes its project root at startup, so refusing paths
/// outside that root is a basic sandbox guarantee.
fn resolve_index_root(
    path: Option<&str>,
    project_root: &std::path::Path,
) -> Result<std::path::PathBuf, McpError> {
    let Some(p) = path else {
        return Ok(project_root.to_path_buf());
    };
    let canonical = std::path::Path::new(p)
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
    Ok(canonical)
}

/// Handle the `index` tool: scan and index the codebase, optionally
/// with a progress callback. Pass `progress = None` for a silent run.
// qual:api
pub fn handle_index_with_progress(
    path: Option<&str>,
    project_root: &std::path::Path,
    progress: Option<&ProgressCallback>,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let root = resolve_index_root(path, project_root)?;

    match RlmSession::index_project(&root, progress) {
        Ok(output) => Ok(RlmServer::success_text(
            formatter,
            RlmServer::to_json(&output),
        )),
        Err(e) => Ok(RlmServer::error_text(formatter, e.to_string())),
    }
}
