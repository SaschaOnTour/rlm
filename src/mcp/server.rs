//! MCP server implementation using rmcp.
//!
//! Exposes all rlm functionality as MCP tools over stdio transport.
//! Each `#[tool]` method is a two-liner: open a [`RlmSession`] for
//! the current project (or bail with a nice error if no index), then
//! call the matching handler in `tool_handlers*`.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, NumberOrString, ProgressNotificationParam, ProgressToken, ServerCapabilities,
    ServerInfo,
};
use rmcp::{
    tool, tool_handler, tool_router, ErrorData as McpError, Peer, RoleServer, ServerHandler,
};

use super::tool_handlers;
use super::tool_handlers_util;
use super::tools::{
    ContextParams, DeleteParams, DepsParams, DiffParams, ExtractParams, FilesParams, IndexParams,
    InsertParams, OverviewParams, PartitionParams, QualityParams, ReadParams, RefsParams,
    ReplaceParams, ScopeParams, SearchParams, StatsParams, SummarizeParams, VerifyParams,
};

/// Default maximum number of search results when no explicit limit is provided.
const DEFAULT_SEARCH_LIMIT: usize = 20;

/// Bounded capacity of the index-progress channel. Small by design: the sender
/// already throttles to 1-in-`PROGRESS_INTERVAL` files, so this only needs to
/// absorb short bursts when `notify_progress` is slower than indexing.
const PROGRESS_CHANNEL_CAPACITY: usize = 16;

use crate::application::query::stats::QualityFlags;
use crate::output::{Formatter, PROGRESS_INTERVAL};

// Re-export start_mcp_server from the helpers module.
pub use super::server_helpers::start_mcp_server;

/// The RLM MCP Server.
///
/// Holds the project root path and the output formatter. A fresh
/// [`RlmSession`](crate::application::session::RlmSession) is opened
/// per tool call via [`Self::ensure_session`] so every request sees a
/// current index and no global state is shared between requests.
// qual:allow(srp) reason: "rmcp #[tool_router] requires all tools on single struct"
#[derive(Clone)]
pub struct RlmServer {
    project_root: PathBuf,
    formatter: Formatter,
    tool_router: Arc<ToolRouter<Self>>,
}

impl RlmServer {
    /// Get the project root path.
    pub(crate) fn project_root(&self) -> &PathBuf {
        &self.project_root
    }

    /// Get access to the tool router for testing purposes.
    // qual:api
    pub fn get_tool_router(&self) -> &ToolRouter<Self> {
        &self.tool_router
    }
}

// -- Tool implementations (thin wrappers) ------------------------------------

#[tool_router]
impl RlmServer {
    #[must_use]
    pub fn new(project_root: PathBuf, formatter: Formatter) -> Self {
        Self {
            project_root,
            formatter,
            tool_router: Arc::new(Self::tool_router()),
        }
    }

    #[tool(
        description = "Scan and index the codebase into the .rlm/index.db database. Returns file/chunk/ref counts."
    )]
    async fn index(
        &self,
        params: Parameters<IndexParams>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let project_root = self.project_root.clone();
        let formatter = self.formatter;
        let path = params.0.path.clone();

        // Bounded channel + throttle-at-source: the callback only sends every
        // PROGRESS_INTERVAL files (and on the final file). A small bounded buffer
        // caps memory if notify_progress is slower than indexing — excess updates
        // are dropped via try_send rather than piling up on the heap.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(usize, usize)>(PROGRESS_CHANNEL_CAPACITY);

        let mut handle = tokio::task::spawn_blocking(move || {
            let progress = move |current: usize, total: usize| {
                if current.is_multiple_of(PROGRESS_INTERVAL) || current == total {
                    let _ = tx.try_send((current, total));
                }
            };
            tool_handlers::handle_index_with_progress(
                path.as_deref(),
                &project_root,
                Some(&progress),
                formatter,
            )
        });

        let token_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let progress_token = ProgressToken(NumberOrString::Number(token_id));
        loop {
            tokio::select! {
                biased;
                msg = rx.recv() => {
                    match msg {
                        Some((current, total)) => {
                            let _ = peer.notify_progress(ProgressNotificationParam {
                                progress_token: progress_token.clone(),
                                progress: current as f64,
                                total: Some(total as f64),
                                message: Some(format!("Indexing... {current}/{total} files")),
                            }).await;
                        }
                        None => break,
                    }
                }
                result = &mut handle => {
                    return result.map_err(|e| McpError::internal_error(e.to_string(), None))?;
                }
            }
        }

        handle
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
    }

    #[tool(
        description = "Full-text search across indexed chunks (symbols and content). Returns matching chunks with content.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn search(&self, params: Parameters<SearchParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers::handle_search(
            &session,
            &params.0.query,
            params.0.limit.unwrap_or(DEFAULT_SEARCH_LIMIT),
            params.0.fields.as_deref(),
            self.formatter,
        )
    }

    #[tool(
        description = "Read a specific symbol (function, struct, etc.) or markdown section from a file. Requires 'symbol' or 'section'. Use metadata=true with symbol to include kind/signature/visibility/call-count. For full-file or line-range reads, use Claude Code's native Read tool.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn read(&self, params: Parameters<ReadParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers::handle_read(&session, &params.0, self.formatter)
    }

    #[tool(
        description = "Project structure overview at three detail levels. 'minimal': symbol names/kinds/lines only (~50 tokens). 'standard' (default): file map with language, line count, public symbols, descriptions. 'tree': directory hierarchy with symbol annotations. Optional path prefix filter.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn overview(
        &self,
        params: Parameters<OverviewParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        let detail = params.0.detail.as_deref().unwrap_or("standard");
        tool_handlers::handle_overview(&session, detail, params.0.path.as_deref(), self.formatter)
    }

    #[tool(
        description = "Find all usages of a symbol and analyze impact: shows every location that would need updating if the symbol changes. Returns file, containing symbol, line, and reference kind.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn refs(&self, params: Parameters<RefsParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers::handle_refs(&session, &params.0.symbol, self.formatter)
    }

    #[tool(
        description = "Replace an AST node (function, struct, etc.) by symbol name. Validates syntax before writing. Use preview=true to see diff without writing."
    )]
    // qual:api
    async fn replace(&self, params: Parameters<ReplaceParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers::handle_replace(&session, &params.0, self.formatter)
    }

    #[tool(
        description = "Delete an AST node by symbol name. Swallows the trailing newline so no orphan blank line remains. Validates syntax before writing."
    )]
    // qual:api
    async fn delete(&self, params: Parameters<DeleteParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers::handle_delete(&session, &params.0, self.formatter)
    }

    #[tool(
        description = "Move one or more symbols from one file to another in a single atomic call. Destination is created if missing, appended to otherwise. Doc-comments and attributes travel with the symbol."
    )]
    // qual:api
    async fn extract(&self, params: Parameters<ExtractParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers::handle_extract(&session, &params.0, self.formatter)
    }

    #[tool(
        description = "Insert code into a file at a specified position (top, bottom, before:N, after:N). Validates syntax before writing."
    )]
    // qual:api
    // qual:allow(srp) reason: "rmcp #[tool_router] requires &self on all #[tool] methods"
    async fn insert(&self, params: Parameters<InsertParams>) -> Result<CallToolResult, McpError> {
        // Insert uniquely allows "no index yet" — session may be None.
        let session = self.try_open_session();
        let p = &params.0;
        let input = tool_handlers::InsertInput {
            path: &p.path,
            position: &p.position,
            code: &p.code,
        };
        tool_handlers::handle_insert(session.as_ref(), &input, &self.project_root, self.formatter)
    }

    #[tool(
        description = "Indexing summary or token-savings report. Default (savings=false): file count, chunk count, reference count, total bytes, language breakdown, and index age. With savings=true: shows how many tokens rlm saved vs Claude Code's native tools. Optional 'since' (ISO 8601) filters the savings window.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn stats(&self, params: Parameters<StatsParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers_util::handle_stats(
            &session,
            params.0.savings.unwrap_or(false),
            params.0.since.as_deref(),
            self.formatter,
        )
    }

    #[tool(
        description = "Inspect parse-quality issues logged during indexing. Flags: unknown_only (only issues without a regression test), all (known + unknown), clear (truncate the log), summary (counts by language / issue type).",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn quality(&self, params: Parameters<QualityParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        let flags = QualityFlags {
            unknown_only: params.0.unknown_only.unwrap_or(false),
            all: params.0.all.unwrap_or(false),
            clear: params.0.clear.unwrap_or(false),
            summary: params.0.summary.unwrap_or(false),
        };
        tool_handlers_util::handle_quality(&session, flags, self.formatter)
    }

    #[tool(
        description = "Split a file into chunks using a strategy: 'semantic' (AST boundaries), 'uniform:N' (N lines each), or 'keyword:PATTERN' (regex split).",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn partition(
        &self,
        params: Parameters<PartitionParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers_util::handle_partition(
            &session,
            &params.0.path,
            &params.0.strategy,
            self.formatter,
        )
    }

    #[tool(
        description = "Generate a condensed summary of a file: language, line count, symbols with descriptions.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn summarize(
        &self,
        params: Parameters<SummarizeParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers_util::handle_summarize(&session, &params.0.path, self.formatter)
    }

    #[tool(
        description = "Compare the indexed version of a file/symbol with the current disk version. Shows if content has changed since last index.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn diff(&self, params: Parameters<DiffParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers_util::handle_diff(
            &session,
            &params.0.path,
            params.0.symbol.as_deref(),
            self.formatter,
        )
    }

    #[tool(
        description = "Complete understanding of a symbol: body content, signatures, caller count, and callee names. Use graph=true to include full callgraph with caller/callee names.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn context(&self, params: Parameters<ContextParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers_util::handle_context(
            &session,
            &params.0.symbol,
            params.0.graph.unwrap_or(false),
            self.formatter,
        )
    }

    #[tool(
        description = "File dependency analysis: lists all imports/use declarations found in the specified file.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn deps(&self, params: Parameters<DepsParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers_util::handle_deps(&session, &params.0.path, self.formatter)
    }

    #[tool(
        description = "Show what symbols are visible at a specific line in a file. Lists containing scopes and all symbols defined before that line.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn scope(&self, params: Parameters<ScopeParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers_util::handle_scope(&session, &params.0.path, params.0.line, self.formatter)
    }

    #[tool(
        description = "List ALL files in the project (indexed + skipped). Unlike overview/search, this shows files with unsupported extensions (.cshtml, .kt, etc.). Use skipped_only=true to find files that need your own tools.",
        annotations(read_only_hint = true)
    )]
    // qual:api
    async fn files(&self, params: Parameters<FilesParams>) -> Result<CallToolResult, McpError> {
        let p = &params.0;
        // `files` intentionally does NOT open a session — it works on
        // unindexed projects too, since its whole point is surfacing
        // files rlm didn't index.
        tool_handlers::handle_files(
            &self.project_root,
            p.path.clone(),
            p.skipped_only.unwrap_or(false),
            p.indexed_only.unwrap_or(false),
            self.formatter,
        )
    }

    #[tool(
        description = "Verify index integrity. Checks for SQLite corruption, orphan chunks/refs, and files that no longer exist on disk. Use fix=true to auto-repair issues."
    )]
    // qual:api
    async fn verify(&self, params: Parameters<VerifyParams>) -> Result<CallToolResult, McpError> {
        let session = self.ensure_session()?;
        tool_handlers_util::handle_verify(&session, params.0.fix.unwrap_or(false), self.formatter)
    }

    #[tool(
        description = "List all supported file extensions with their language and parser type (tree-sitter, structural, semantic, plaintext).",
        annotations(read_only_hint = true)
    )]
    // qual:api
    // qual:allow(srp) reason: "rmcp #[tool_router] requires &self on all #[tool] methods"
    async fn supported(&self) -> Result<CallToolResult, McpError> {
        tool_handlers_util::handle_supported(self.formatter)
    }
}

// -- ServerHandler implementation --------------------------------------------

#[tool_handler]
impl ServerHandler for RlmServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "rlm: Context Broker for semantic code exploration. 20 tools in 4 tiers:\n\
                 ORIENT: overview(detail='minimal'|'standard'|'tree', path?) — project structure at 3 zoom levels.\n\
                 SEARCH: search(query) — full-text across symbols. read(path, symbol|section, metadata?) — symbol body + optional type/signature enrichment.\n\
                 ANALYZE: refs(symbol) — all usages + impact analysis. context(symbol, graph?) — body + callers + callees. deps(path), scope(path, line).\n\
                 EDIT: replace(path, symbol, code, preview?), delete(path, symbol, keep_docs?), insert(path, code, position), extract(path, symbols, to) — Syntax Guard validates all writes.\n\
                 UTILITY: diff, partition, summarize, files, stats(savings?, since?), quality(unknown_only?, all?, clear?, summary?), verify, supported, index.\n\
                 IMPORTANT: 'read' requires symbol or section. Use Claude Code's Read for full files/line ranges.\n\
                 Check 'q' field: if 'fallback_recommended' is true, prefer Claude Code's Read for affected lines."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            ..Default::default()
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
