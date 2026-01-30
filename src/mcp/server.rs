//! MCP server implementation using rmcp.
//!
//! Exposes all rlm functionality as MCP tools over stdio transport.
//! Each tool calls the same core logic as the CLI commands.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt};
use serde::Serialize;

use crate::config::Config;
use crate::db::Database;
use crate::edit::syntax_guard::SyntaxGuard;
use crate::edit::{inserter, replacer};
use crate::indexer;
use crate::operations::{self, parse_position};
use crate::rlm::{batch, grep, partition, peek, summarize};
use crate::search::tree;

use super::tools::{
    BatchParams, CallgraphParams, ContextParams, DepsParams, DiffParams, FilesParams, GrepParams,
    ImpactParams, IndexParams, InsertParams, MapParams, PartitionParams, PatternsParams,
    PeekParams, ReadParams, RefsParams, ReplaceParams, ScopeParams, SearchParams, SignatureParams,
    SummarizeParams, TypeParams, VerifyParams,
};

/// The RLM MCP Server.
///
/// Holds the project root path. The database is opened on-demand for each tool
/// call to avoid lifetime issues with the sqlite connection.
#[derive(Clone)]
pub struct RlmServer {
    project_root: PathBuf,
    tool_router: Arc<ToolRouter<Self>>,
}

// ── Helper functions ────────────────────────────────────────────

impl RlmServer {
    fn config(&self) -> Config {
        Config::new(&self.project_root)
    }

    /// Get the database. Returns an error if the index doesn't exist.
    /// Unlike the CLI, MCP does NOT auto-index to avoid blocking on large projects.
    fn ensure_db(&self) -> Result<Database, McpError> {
        let config = self.config();
        if !config.index_exists() {
            return Err(McpError::invalid_request(
                "Index not found. Run 'rlm index .' first before using MCP tools.",
                None,
            ));
        }
        Database::open(&config.db_path)
            .map_err(|e| McpError::internal_error(format!("database error: {e}"), None))
    }

    fn to_json<T: Serialize>(val: &T) -> String {
        serde_json::to_string(val).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
    }

    fn success_text(text: String) -> CallToolResult {
        CallToolResult::success(vec![Content::text(text)])
    }

    fn error_text(msg: String) -> CallToolResult {
        CallToolResult::success(vec![Content::text(format!("{{\"error\":\"{msg}\"}}"))])
    }

    /// Get access to the tool router for testing purposes.
    pub fn get_tool_router(&self) -> &ToolRouter<Self> {
        &self.tool_router
    }
}

// ── Tool implementations ────────────────────────────────────────

#[tool_router]
impl RlmServer {
    #[must_use]
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            tool_router: Arc::new(Self::tool_router()),
        }
    }

    // ─── Indexing ───────────────────────────────────────────────

    #[tool(
        description = "Scan and index the codebase into the .rlm/index.db database. Returns file/chunk/ref counts."
    )]
    async fn index(&self, params: Parameters<IndexParams>) -> Result<CallToolResult, McpError> {
        let config = if let Some(path) = &params.0.path {
            Config::new(path)
        } else {
            self.config()
        };

        if let Err(e) = config.ensure_rlm_dir() {
            return Ok(Self::error_text(e.to_string()));
        }

        match indexer::run_index(&config) {
            Ok(result) => {
                let output: operations::IndexOutput = result.into();
                Ok(Self::success_text(Self::to_json(&output)))
            }
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Search ─────────────────────────────────────────────────

    #[tool(
        description = "Full-text search across indexed chunks (symbols and content). Returns matching chunks with content."
    )]
    async fn search(&self, params: Parameters<SearchParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let limit = params.0.limit.unwrap_or(20);

        match operations::search_chunks(&db, &params.0.query, limit) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Read ───────────────────────────────────────────────────

    #[tool(
        description = "Read file content. Can read full file, a specific symbol, a markdown section, or a line range."
    )]
    async fn read(&self, params: Parameters<ReadParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let config = self.config();
        let params = &params.0;

        if let Some(sym) = &params.symbol {
            match db.get_chunks_by_ident(sym) {
                Ok(chunks) => {
                    let file_chunks: Vec<_> = chunks
                        .iter()
                        .filter(|c| {
                            db.get_all_files().ok().is_some_and(|files| {
                                files
                                    .iter()
                                    .any(|f| f.id == c.file_id && f.path == params.path)
                            })
                        })
                        .collect();

                    if file_chunks.is_empty() {
                        if chunks.is_empty() {
                            return Ok(Self::error_text(format!("symbol not found: {sym}")));
                        }
                        Ok(Self::success_text(Self::to_json(&chunks)))
                    } else {
                        Ok(Self::success_text(Self::to_json(&file_chunks)))
                    }
                }
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        } else if let Some(heading) = &params.section {
            match db.get_file_by_path(&params.path) {
                Ok(Some(file)) => match db.get_chunks_for_file(file.id) {
                    Ok(chunks) => match chunks.iter().find(|c| c.ident == *heading) {
                        Some(c) => Ok(Self::success_text(Self::to_json(c))),
                        None => Ok(Self::error_text(format!("section not found: {heading}"))),
                    },
                    Err(e) => Ok(Self::error_text(e.to_string())),
                },
                Ok(None) => Ok(Self::error_text(format!("file not found: {}", params.path))),
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        } else if let Some(line_range) = &params.lines {
            let full_path = config.project_root.join(&params.path);
            match std::fs::read_to_string(&full_path) {
                Ok(source) => {
                    let all_lines: Vec<&str> = source.lines().collect();
                    let parts: Vec<&str> = line_range.split('-').collect();
                    if parts.len() != 2 {
                        return Ok(Self::error_text("line range must be START-END".into()));
                    }
                    let start: usize = parts[0].parse::<usize>().unwrap_or(1).saturating_sub(1);
                    let end: usize = parts[1]
                        .parse()
                        .unwrap_or(all_lines.len())
                        .min(all_lines.len());
                    let content = all_lines[start..end].join("\n");
                    Ok(Self::success_text(Self::to_json(&content)))
                }
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        } else {
            let full_path = config.project_root.join(&params.path);
            match std::fs::read_to_string(&full_path) {
                Ok(content) => Ok(Self::success_text(Self::to_json(&content))),
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        }
    }

    // ─── Tree ───────────────────────────────────────────────────

    #[tool(
        description = "Display the folder structure with symbol annotations. Shows files and their contained symbols hierarchically."
    )]
    async fn tree(&self) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        match tree::build_tree(&db) {
            Ok(nodes) => Ok(Self::success_text(Self::to_json(&nodes))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Refs ───────────────────────────────────────────────────

    #[tool(
        description = "Find all usages/call sites of a symbol across the codebase. Returns reference locations with kind (call, import, type_use)."
    )]
    async fn refs(&self, params: Parameters<RefsParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::get_refs(&db, &params.0.symbol) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Signature ──────────────────────────────────────────────

    #[tool(
        description = "Get the signature of a symbol plus the count of all call sites. Useful for refactoring."
    )]
    async fn signature(
        &self,
        params: Parameters<SignatureParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::get_signature(&db, &params.0.symbol) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Replace ────────────────────────────────────────────────

    #[tool(
        description = "Replace an AST node (function, struct, etc.) by symbol name. Validates syntax before writing. Use preview=true to see diff without writing."
    )]
    async fn replace(&self, params: Parameters<ReplaceParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let params = &params.0;

        if params.preview.unwrap_or(false) {
            match replacer::preview_replace(&db, &params.path, &params.symbol, &params.code) {
                Ok(diff) => {
                    #[derive(Serialize)]
                    struct Out {
                        file: String,
                        symbol: String,
                        old_lines: (u32, u32),
                        old_code: String,
                        new_code: String,
                    }
                    Ok(Self::success_text(Self::to_json(&Out {
                        file: diff.file,
                        symbol: diff.symbol,
                        old_lines: (diff.start_line, diff.end_line),
                        old_code: diff.old_code,
                        new_code: diff.new_code,
                    })))
                }
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        } else {
            let guard = SyntaxGuard::new();
            match replacer::replace_symbol(&db, &params.path, &params.symbol, &params.code, &guard)
            {
                Ok(_) => Ok(Self::success_text("{\"ok\":true}".to_string())),
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        }
    }

    // ─── Insert ─────────────────────────────────────────────────

    #[tool(
        description = "Insert code into a file at a specified position (top, bottom, before:N, after:N). Validates syntax before writing."
    )]
    async fn insert(&self, params: Parameters<InsertParams>) -> Result<CallToolResult, McpError> {
        let params = &params.0;
        let pos = match parse_position(&params.position) {
            Ok(p) => p,
            Err(e) => return Ok(Self::error_text(e.to_string())),
        };

        let guard = SyntaxGuard::new();
        match inserter::insert_code(&params.path, &pos, &params.code, &guard) {
            Ok(_) => Ok(Self::success_text("{\"ok\":true}".to_string())),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Stats ──────────────────────────────────────────────────

    #[tool(
        description = "Get indexing statistics: file count, chunk count, reference count, total bytes, language breakdown, and index age."
    )]
    async fn stats(&self) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::get_stats(&db) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Peek (RLM) ────────────────────────────────────────────

    #[tool(
        description = "Quick structure preview - shows symbols with their kinds and line counts, NO content. Cheapest operation (~50 tokens)."
    )]
    async fn peek(&self, params: Parameters<PeekParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        match peek::peek(&db, params.0.path.as_deref()) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Grep (RLM) ────────────────────────────────────────────

    #[tool(
        description = "Pattern-based search using regex. Returns matching lines with optional context. Use for targeted content finding."
    )]
    async fn grep(&self, params: Parameters<GrepParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let config = self.config();
        let params = &params.0;
        let context = params.context.unwrap_or(0);
        match grep::grep(
            &db,
            &params.pattern,
            context,
            params.path.as_deref(),
            &config.project_root,
        ) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Partition (RLM) ────────────────────────────────────────

    #[tool(
        description = "Split a file into chunks using a strategy: 'semantic' (AST boundaries), 'uniform:N' (N lines each), or 'keyword:PATTERN' (regex split)."
    )]
    async fn partition(
        &self,
        params: Parameters<PartitionParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let config = self.config();
        let params = &params.0;

        let strategy = if params.strategy == "semantic" {
            partition::Strategy::Semantic
        } else if params.strategy.starts_with("uniform:") {
            match params.strategy[8..].parse::<usize>() {
                Ok(n) => partition::Strategy::Uniform(n),
                Err(e) => return Ok(Self::error_text(format!("invalid chunk size: {e}"))),
            }
        } else if params.strategy.starts_with("keyword:") {
            partition::Strategy::Keyword(params.strategy[8..].to_string())
        } else {
            return Ok(Self::error_text(
                "strategy must be: semantic, uniform:N, or keyword:PATTERN".into(),
            ));
        };

        match partition::partition_file(&db, &params.path, &strategy, &config.project_root) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Summarize (RLM) ───────────────────────────────────────

    #[tool(
        description = "Generate a condensed summary of a file: language, line count, symbols with descriptions."
    )]
    async fn summarize(
        &self,
        params: Parameters<SummarizeParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        match summarize::summarize(&db, &params.0.path) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Batch (RLM) ───────────────────────────────────────────

    #[tool(
        description = "Run a search query across all indexed files. Returns results grouped by file."
    )]
    async fn batch(&self, params: Parameters<BatchParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let limit = params.0.limit.unwrap_or(20);
        match batch::batch_search(&db, &params.0.query, limit) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Diff (RLM) ────────────────────────────────────────────

    #[tool(
        description = "Compare the indexed version of a file/symbol with the current disk version. Shows if content has changed since last index."
    )]
    async fn diff(&self, params: Parameters<DiffParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let config = self.config();
        let params = &params.0;

        if let Some(sym) = &params.symbol {
            match operations::diff_symbol(&db, &params.path, sym, &config.project_root) {
                Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        } else {
            match operations::diff_file(&db, &params.path, &config.project_root) {
                Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        }
    }

    // ─── Map ────────────────────────────────────────────────────

    #[tool(
        description = "Project overview: for each file shows language, line count, public symbols, and a brief description. One-call orientation."
    )]
    async fn map(&self, params: Parameters<MapParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::build_map(&db, params.0.path.as_deref()) {
            Ok(entries) => Ok(Self::success_text(Self::to_json(&entries))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Callgraph ──────────────────────────────────────────────

    #[tool(
        description = "Build call graph for a symbol: who calls it (callers) and what it calls (callees). Returns directed graph edges."
    )]
    async fn callgraph(
        &self,
        params: Parameters<CallgraphParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::build_callgraph(&db, &params.0.symbol) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Impact ─────────────────────────────────────────────────

    #[tool(
        description = "Impact analysis: shows all locations that would need updating if a symbol changes. Lists file, containing symbol, line, and reference kind."
    )]
    async fn impact(&self, params: Parameters<ImpactParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::analyze_impact(&db, &params.0.symbol) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Context ────────────────────────────────────────────────

    #[tool(
        description = "Complete understanding of a symbol: body content, signatures, caller count, and callee names. One-call deep understanding."
    )]
    async fn context(&self, params: Parameters<ContextParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::build_context(&db, &params.0.symbol) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Deps ───────────────────────────────────────────────────

    #[tool(
        description = "File dependency analysis: lists all imports/use declarations found in the specified file."
    )]
    async fn deps(&self, params: Parameters<DepsParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::get_deps(&db, &params.0.path) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Scope ──────────────────────────────────────────────────

    #[tool(
        description = "Show what symbols are visible at a specific line in a file. Lists containing scopes and all symbols defined before that line."
    )]
    async fn scope(&self, params: Parameters<ScopeParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::get_scope(&db, &params.0.path, params.0.line) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Type ───────────────────────────────────────────────────

    #[tool(
        description = "Get type information for a symbol: kind (fn/struct/class/etc.), signature, and full content."
    )]
    async fn type_info(&self, params: Parameters<TypeParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::get_type_info(&db, &params.0.symbol) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Patterns ───────────────────────────────────────────────

    #[tool(
        description = "Find similar implementations in the codebase. Returns matching symbols with their kind, signature, and line count."
    )]
    async fn patterns(
        &self,
        params: Parameters<PatternsParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::find_patterns(&db, &params.0.query) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Files ─────────────────────────────────────────────────────

    #[tool(
        description = "List ALL files in the project (indexed + skipped). Unlike map/tree/search, this shows files with unsupported extensions (.cshtml, .kt, etc.). Use skipped_only=true to find files that need your own tools."
    )]
    async fn files(&self, params: Parameters<FilesParams>) -> Result<CallToolResult, McpError> {
        let params = &params.0;
        let filter = operations::FilesFilter {
            path_prefix: params.path.clone(),
            skipped_only: params.skipped_only.unwrap_or(false),
            indexed_only: params.indexed_only.unwrap_or(false),
        };

        match operations::list_files(&self.project_root, filter) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // ─── Verify ────────────────────────────────────────────────────

    #[tool(
        description = "Verify index integrity. Checks for SQLite corruption, orphan chunks/refs, and files that no longer exist on disk. Use fix=true to auto-repair issues."
    )]
    async fn verify(&self, params: Parameters<VerifyParams>) -> Result<CallToolResult, McpError> {
        let config = self.config();

        if !config.index_exists() {
            return Ok(Self::error_text(
                "Index not found. Run 'rlm index' first.".into(),
            ));
        }

        let db = crate::db::Database::open(&config.db_path)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let report = match operations::verify_index(&db, &config.project_root) {
            Ok(r) => r,
            Err(e) => return Ok(Self::error_text(e.to_string())),
        };

        if params.0.fix.unwrap_or(false) && !report.is_ok() {
            match operations::fix_integrity(&db, &report) {
                Ok(fix_result) => Ok(Self::success_text(Self::to_json(&fix_result))),
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        } else {
            Ok(Self::success_text(Self::to_json(&report)))
        }
    }

    // ─── Supported ─────────────────────────────────────────────────

    #[tool(
        description = "List all supported file extensions with their language and parser type (tree-sitter, structural, semantic, plaintext)."
    )]
    async fn supported(&self) -> Result<CallToolResult, McpError> {
        Ok(Self::success_text(Self::to_json(
            &operations::list_supported(),
        )))
    }
}

// ── ServerHandler implementation ────────────────────────────────

#[tool_handler]
impl ServerHandler for RlmServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "rlm: The Context Broker for semantic code exploration. \
                 Use progressive disclosure: peek -> grep -> map -> tree -> search -> read. \
                 For code intelligence: refs, signature, callgraph, impact, context, deps, scope, type, patterns. \
                 For editing: replace (swap AST node), insert (add code). Syntax Guard validates all writes. \
                 Indexing respects .gitignore and excludes hidden files and common build directories. \
                 IMPORTANT: Most tools (tree, map, search, refs, etc.) only show files with supported extensions. \
                 To see ALL files including skipped ones (.cshtml, .kt, etc.), use the 'files' tool. \
                 To see only skipped files: files(skipped_only=true). \
                 IMPORTANT: Check the 'q' field in responses. If 'fallback_recommended' is true, \
                 the file contains syntax that couldn't be fully parsed (e.g. Java records, Python match). \
                 In that case, prefer 'read --lines' or 'grep' over AST-based commands for affected lines."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            ..Default::default()
        }
    }
}

// ── Server startup ──────────────────────────────────────────────

/// Start the MCP server on stdio transport.
pub async fn start_mcp_server() -> crate::error::Result<()> {
    // Initialize tracing to stderr (stdout is the MCP transport)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting rlm MCP server");

    // Determine project root from current working directory
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let server = RlmServer::new(project_root);

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| crate::error::RlmError::Other(format!("MCP server error: {e}")))?;

    tracing::info!("MCP server running on stdio");

    service
        .waiting()
        .await
        .map_err(|e| crate::error::RlmError::Other(format!("MCP server error: {e}")))?;

    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════════
// Unit Tests - verify tool implementations call correct operations
// ══════════════════════════════════════════════════════════════════════════════
//
// These tests verify that each MCP tool:
// 1. Calls the correct underlying operation
// 2. Returns results in the expected format
// 3. Properly handles errors
//
// Note: Since rmcp's Peer::new is pub(crate), we test the underlying operations
// directly rather than going through the ToolRouter. The tool registration and
// schema tests are in tests/mcp_tests.rs.

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Setup: create temp dir with test file and index it
    fn setup_indexed_project() -> (TempDir, Config, Database) {
        let tmp = TempDir::new().expect("create tempdir");

        std::fs::write(
            tmp.path().join("test.rs"),
            r#"/// A test struct for configuration.
pub struct Config {
    pub name: String,
    pub value: i32,
}

impl Config {
    pub fn new(name: String, value: i32) -> Self {
        Self { name, value }
    }
}

/// Helper function that doubles the input.
pub fn helper(x: i32) -> i32 {
    x * 2
}

fn internal() {
    let _cfg = Config::new("test".into(), 42);
    let _result = helper(10);
}
"#,
        )
        .expect("write test file");

        let config = Config::new(tmp.path());
        crate::indexer::run_index(&config).expect("index project");
        let db = Database::open(&config.db_path).expect("open db");

        (tmp, config, db)
    }

    // ─── Verify tool methods call correct operations ────────────────────────
    //
    // Each test verifies that calling the corresponding operation produces
    // output that matches what the tool should return.

    #[test]
    fn test_stats_operation_returns_expected_format() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::get_stats(&db).expect("get_stats");

        // Stats should have file/chunk/ref counts
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("files"), "stats should have files");
        assert!(json.contains("chunks"), "stats should have chunks");
        assert!(json.contains("refs"), "stats should have refs");
    }

    #[test]
    fn test_search_operation_returns_matching_chunks() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::search_chunks(&db, "Config", 10).expect("search");

        // Search should find Config struct
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Config"), "search should find Config");
    }

    #[test]
    fn test_peek_operation_returns_structure() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = peek::peek(&db, None).expect("peek");

        // Peek should show file structure
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test.rs"), "peek should show test.rs");
    }

    #[test]
    fn test_tree_operation_returns_hierarchy() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = tree::build_tree(&db).expect("tree");

        // Tree should have nodes
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test.rs"), "tree should show test.rs");
    }

    #[test]
    fn test_grep_operation_returns_matches() {
        let (tmp, _config, db) = setup_indexed_project();
        let result = grep::grep(&db, "helper", 0, None, tmp.path()).expect("grep");

        // Grep should find matches
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("helper"), "grep should find helper");
    }

    #[test]
    fn test_refs_operation_returns_references() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::get_refs(&db, "helper").expect("refs");

        // Refs should return reference info
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("helper"), "refs should reference helper");
    }

    #[test]
    fn test_signature_operation_returns_signature_info() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::get_signature(&db, "helper").expect("signature");

        // Signature should have symbol info
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("helper"), "signature should show helper");
    }

    #[test]
    fn test_map_operation_returns_file_overview() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::build_map(&db, None).expect("map");

        // Map should show files
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test.rs"), "map should show test.rs");
    }

    #[test]
    fn test_callgraph_operation_returns_graph() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::build_callgraph(&db, "helper").expect("callgraph");

        // Callgraph should have symbol info
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("helper"), "callgraph should show helper");
    }

    #[test]
    fn test_impact_operation_returns_affected_locations() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::analyze_impact(&db, "helper").expect("impact");

        // Impact should show affected locations
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("helper"), "impact should show helper");
    }

    #[test]
    fn test_context_operation_returns_full_context() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::build_context(&db, "helper").expect("context");

        // Context should have body
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("body"), "context should have body");
        assert!(json.contains("helper"), "context should show helper");
    }

    #[test]
    fn test_deps_operation_returns_dependencies() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::get_deps(&db, "test.rs").expect("deps");

        // Deps should return (possibly empty) list
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test.rs"), "deps should show file");
    }

    #[test]
    fn test_scope_operation_returns_visible_symbols() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::get_scope(&db, "test.rs", 15).expect("scope");

        // Scope should show file info
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test.rs"), "scope should show file");
    }

    #[test]
    fn test_type_info_operation_returns_type_details() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::get_type_info(&db, "Config").expect("type_info");

        // Type should show struct info
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Config"), "type should show Config");
        assert!(json.contains("struct"), "type should show struct kind");
    }

    #[test]
    fn test_patterns_operation_returns_similar_implementations() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::find_patterns(&db, "new").expect("patterns");

        // Patterns should find implementations
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            json.contains("new") || json.contains("Config"),
            "patterns should find implementations"
        );
    }

    #[test]
    fn test_partition_semantic_operation_returns_chunks() {
        let (tmp, _config, db) = setup_indexed_project();
        let result =
            partition::partition_file(&db, "test.rs", &partition::Strategy::Semantic, tmp.path())
                .expect("partition");

        // Partition should return chunks
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.is_empty(), "partition should return chunks");
    }

    #[test]
    fn test_summarize_operation_returns_summary() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = summarize::summarize(&db, "test.rs").expect("summarize");

        // Summarize should have file info
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.is_empty(), "summarize should return content");
    }

    #[test]
    fn test_batch_operation_returns_search_results() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = batch::batch_search(&db, "Config", 10).expect("batch");

        // Batch should find results
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            json.contains("Config") || json.contains("test.rs"),
            "batch should find results"
        );
    }

    #[test]
    fn test_diff_operation_returns_change_status() {
        let (tmp, _config, db) = setup_indexed_project();
        let result = operations::diff_file(&db, "test.rs", tmp.path()).expect("diff");

        // Diff should show change status
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("changed"), "diff should show changed status");
    }

    #[test]
    fn test_verify_operation_returns_integrity_report() {
        let (tmp, _config, db) = setup_indexed_project();
        let result = operations::verify_index(&db, tmp.path()).expect("verify");

        // Verify should return report
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            json.contains("ok") || json.contains("sqlite"),
            "verify should return report"
        );
    }

    #[test]
    fn test_files_operation_returns_file_list() {
        let (tmp, _config, _db) = setup_indexed_project();
        let filter = operations::FilesFilter::default();
        let result = operations::list_files(tmp.path(), filter).expect("files");

        // Files should list files
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test.rs"), "files should list test.rs");
    }

    #[test]
    fn test_supported_operation_returns_extensions() {
        let result = operations::list_supported();

        // Supported should list extensions
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("rs"), "should list .rs");
        assert!(json.contains("py"), "should list .py");
        assert!(json.contains("go"), "should list .go");
    }

    // ─── Verify tool methods use correct helper functions ───────────────────

    #[test]
    fn test_ensure_db_fails_without_index() {
        let tmp = TempDir::new().expect("create tempdir");
        let server = RlmServer::new(tmp.path().to_path_buf());

        let result = server.ensure_db();
        assert!(result.is_err(), "ensure_db should fail without index");

        let err = match result {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        assert!(
            format!("{err:?}").contains("Index not found"),
            "error should mention missing index"
        );
    }

    #[test]
    fn test_ensure_db_succeeds_with_index() {
        let (tmp, _config, _db) = setup_indexed_project();
        let server = RlmServer::new(tmp.path().to_path_buf());

        let result = server.ensure_db();
        assert!(result.is_ok(), "ensure_db should succeed with index");
    }

    #[test]
    fn test_to_json_produces_valid_json() {
        #[derive(serde::Serialize)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let json = RlmServer::to_json(&data);
        assert!(json.contains("test"), "JSON should contain name");
        assert!(json.contains("42"), "JSON should contain value");

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["name"], "test");
        assert_eq!(parsed["value"], 42);
    }

    #[test]
    fn test_success_text_creates_correct_result() {
        let result = RlmServer::success_text("test content".to_string());

        assert!(!result.content.is_empty(), "should have content");
        let text = result.content[0].as_text().expect("should be text");
        assert_eq!(text.text, "test content");
    }

    #[test]
    fn test_error_text_creates_json_error() {
        let result = RlmServer::error_text("test error".to_string());

        assert!(!result.content.is_empty(), "should have content");
        let text = result.content[0].as_text().expect("should be text");
        assert!(text.text.contains("error"), "should be error JSON");
        assert!(text.text.contains("test error"), "should contain message");
    }
}
