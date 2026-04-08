//! MCP server implementation using rmcp.
//!
//! Exposes all rlm functionality as MCP tools over stdio transport.
//! Each tool calls the same core logic as the CLI commands.
//!
//! Helper methods and server startup live in `server_helpers`.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use serde::Serialize;

use crate::config::Config;
use crate::edit::syntax_guard::SyntaxGuard;
use crate::edit::{inserter, replacer};
use crate::indexer;
use crate::models::token_estimate::estimate_tokens;
use crate::operations;
use crate::operations::savings;
use crate::rlm::{partition, summarize};
use crate::search::tree;

/// Default maximum number of search results when no explicit limit is provided.
const DEFAULT_SEARCH_LIMIT: usize = 20;

use super::tools::{
    ContextParams, DepsParams, DiffParams, FilesParams, IndexParams, InsertParams, OverviewParams,
    PartitionParams, ReadParams, RefsParams, ReplaceParams, SavingsParams, ScopeParams,
    SearchParams, SummarizeParams, VerifyParams,
};

// Re-export start_mcp_server from the helpers module.
pub use super::server_helpers::start_mcp_server;

/// The RLM MCP Server.
///
/// Holds the project root path. The database is opened on-demand for each tool
/// call to avoid lifetime issues with the sqlite connection.
#[derive(Clone)]
pub struct RlmServer {
    project_root: PathBuf,
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

// -- Tool implementations ----------------------------------------------------

#[tool_router]
impl RlmServer {
    #[must_use]
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            tool_router: Arc::new(Self::tool_router()),
        }
    }

    // --- Indexing ------------------------------------------------------------

    #[tool(
        description = "Scan and index the codebase into the .rlm/index.db database. Returns file/chunk/ref counts."
    )]
    // qual:api
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

    // --- Search -------------------------------------------------------------

    #[tool(
        description = "Full-text search across indexed chunks (symbols and content). Returns matching chunks with content."
    )]
    // qual:api
    async fn search(&self, params: Parameters<SearchParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let limit = params.0.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);

        match operations::search_chunks(&db, &params.0.query, limit) {
            Ok(result) => {
                let json = Self::to_json(&result);
                let out_tokens = estimate_tokens(json.len());
                let alt_tokens = result.tokens.output.max(out_tokens);
                savings::record(
                    &db,
                    "search",
                    out_tokens,
                    alt_tokens,
                    result.results.len() as u64,
                );
                Ok(Self::success_text(json))
            }
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Read (symbol or section only) --------------------------------------

    #[tool(
        description = "Read a specific symbol (function, struct, etc.) or markdown section from a file. Requires 'symbol' or 'section'. Use metadata=true with symbol to include kind/signature/visibility/call-count. For full-file or line-range reads, use Claude Code's native Read tool."
    )]
    // qual:api
    async fn read(&self, params: Parameters<ReadParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let params = &params.0;

        match (&params.symbol, &params.section) {
            (Some(_), _) => self.read_symbol(&db, params),
            (_, Some(_)) => Self::read_section(&db, params),
            _ => Ok(Self::error_text(
                "read requires 'symbol' or 'section'. Use Claude Code's Read for full files or line ranges.".into(),
            )),
        }
    }

    fn read_symbol(
        &self,
        db: &crate::db::Database,
        params: &ReadParams,
    ) -> Result<CallToolResult, McpError> {
        let sym = params.symbol.as_deref().unwrap_or_default();
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

                let target_chunks = if file_chunks.is_empty() {
                    if chunks.is_empty() {
                        return Ok(Self::error_text(format!("symbol not found: {sym}")));
                    }
                    &chunks
                } else {
                    return Self::read_symbol_result(db, params, &file_chunks);
                };

                Self::read_symbol_result(db, params, target_chunks)
            }
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    fn read_section(
        db: &crate::db::Database,
        params: &ReadParams,
    ) -> Result<CallToolResult, McpError> {
        let heading = params.section.as_deref().unwrap_or_default();
        match db.get_file_by_path(&params.path) {
            Ok(Some(file)) => match db.get_chunks_for_file(file.id) {
                Ok(chunks) => match chunks.iter().find(|c| c.ident == *heading) {
                    Some(c) => {
                        let json =
                            savings::record_file_op(db, "read_section", c, &params.path);
                        Ok(Self::success_text(json))
                    }
                    None => Ok(Self::error_text(format!("section not found: {heading}"))),
                },
                Err(e) => Ok(Self::error_text(e.to_string())),
            },
            Ok(None) => Ok(Self::error_text(format!("file not found: {}", params.path))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Overview (consolidated peek/map/tree) ------------------------------

    #[tool(
        description = "Project structure overview at three detail levels. 'minimal': symbol names/kinds/lines only (~50 tokens). 'standard' (default): file map with language, line count, public symbols, descriptions. 'tree': directory hierarchy with symbol annotations. Optional path prefix filter."
    )]
    // qual:api
    async fn overview(
        &self,
        params: Parameters<OverviewParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let detail = params.0.detail.as_deref().unwrap_or("standard");
        let path = params.0.path.as_deref();

        match detail {
            "minimal" => {
                use crate::rlm::peek;
                match peek::peek(&db, path) {
                    Ok(result) => {
                        let json = savings::record_scoped_op(&db, "overview", &result, path);
                        Ok(Self::success_text(json))
                    }
                    Err(e) => Ok(Self::error_text(e.to_string())),
                }
            }
            "standard" => match operations::build_map(&db, path) {
                Ok(entries) => {
                    let json = savings::record_scoped_op(&db, "overview", &entries, path);
                    Ok(Self::success_text(json))
                }
                Err(e) => Ok(Self::error_text(e.to_string())),
            },
            "tree" => match tree::build_tree(&db, path) {
                Ok(nodes) => {
                    let json = savings::record_scoped_op(&db, "overview", &nodes, path);
                    Ok(Self::success_text(json))
                }
                Err(e) => Ok(Self::error_text(e.to_string())),
            },
            other => Ok(Self::error_text(format!(
                "unknown detail level: '{other}'. Use 'minimal', 'standard', or 'tree'."
            ))),
        }
    }

    // --- Refs (enriched with impact analysis) -------------------------------

    #[tool(
        description = "Find all usages of a symbol and analyze impact: shows every location that would need updating if the symbol changes. Returns file, containing symbol, line, and reference kind."
    )]
    // qual:api
    async fn refs(&self, params: Parameters<RefsParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let symbol = &params.0.symbol;

        match operations::analyze_impact(&db, symbol) {
            Ok(result) => {
                let files_touched = result.count as u64;
                let json = savings::record_symbol_op(&db, "refs", &result, symbol, files_touched);
                Ok(Self::success_text(json))
            }
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Replace ------------------------------------------------------------

    #[tool(
        description = "Replace an AST node (function, struct, etc.) by symbol name. Validates syntax before writing. Use preview=true to see diff without writing."
    )]
    // qual:api
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

    // --- Insert -------------------------------------------------------------

    #[tool(
        description = "Insert code into a file at a specified position (top, bottom, before:N, after:N). Validates syntax before writing."
    )]
    // qual:api
    async fn insert(&self, params: Parameters<InsertParams>) -> Result<CallToolResult, McpError> {
        let params = &params.0;
        let guard = SyntaxGuard::new();
        match inserter::insert_code(&params.path, &params.position, &params.code, &guard) {
            Ok(_) => Ok(Self::success_text("{\"ok\":true}".to_string())),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Stats --------------------------------------------------------------

    #[tool(
        description = "Get indexing statistics: file count, chunk count, reference count, total bytes, language breakdown, and index age."
    )]
    // qual:api
    async fn stats(&self) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;

        match operations::get_stats(&db) {
            Ok(result) => Ok(Self::success_text(Self::to_json(&result))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Partition -----------------------------------------------------------

    #[tool(
        description = "Split a file into chunks using a strategy: 'semantic' (AST boundaries), 'uniform:N' (N lines each), or 'keyword:PATTERN' (regex split)."
    )]
    // qual:api
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
            Ok(result) => {
                let json = savings::record_file_op(&db, "partition", &result, &params.path);
                Ok(Self::success_text(json))
            }
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Summarize ----------------------------------------------------------

    #[tool(
        description = "Generate a condensed summary of a file: language, line count, symbols with descriptions."
    )]
    // qual:api
    async fn summarize(
        &self,
        params: Parameters<SummarizeParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let path = &params.0.path;
        let result = summarize::summarize(&db, path);
        self.file_op_result("summarize", path, result)
    }

    // --- Diff ---------------------------------------------------------------

    #[tool(
        description = "Compare the indexed version of a file/symbol with the current disk version. Shows if content has changed since last index."
    )]
    // qual:api
    async fn diff(&self, params: Parameters<DiffParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let config = self.config();
        let params = &params.0;

        if let Some(sym) = &params.symbol {
            match operations::diff_symbol(&db, &params.path, sym, &config.project_root) {
                Ok(result) => {
                    let json = savings::record_file_op(&db, "diff", &result, &params.path);
                    Ok(Self::success_text(json))
                }
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        } else {
            match operations::diff_file(&db, &params.path, &config.project_root) {
                Ok(result) => {
                    let json = savings::record_file_op(&db, "diff", &result, &params.path);
                    Ok(Self::success_text(json))
                }
                Err(e) => Ok(Self::error_text(e.to_string())),
            }
        }
    }

    // --- Context (with optional callgraph) ----------------------------------

    #[tool(
        description = "Complete understanding of a symbol: body content, signatures, caller count, and callee names. Use graph=true to include full callgraph with caller/callee names."
    )]
    // qual:api
    async fn context(&self, params: Parameters<ContextParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let symbol = &params.0.symbol;
        let include_graph = params.0.graph.unwrap_or(false);

        match operations::build_context(&db, symbol) {
            Ok(ctx_result) => {
                if include_graph {
                    match operations::build_callgraph(&db, symbol) {
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
                            let json =
                                savings::record_symbol_op(&db, "context", &combined, symbol, 0);
                            Ok(Self::success_text(json))
                        }
                        Err(e) => Ok(Self::error_text(e.to_string())),
                    }
                } else {
                    let json =
                        savings::record_symbol_op(&db, "context", &ctx_result, symbol, 0);
                    Ok(Self::success_text(json))
                }
            }
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Deps ---------------------------------------------------------------

    #[tool(
        description = "File dependency analysis: lists all imports/use declarations found in the specified file."
    )]
    // qual:api
    async fn deps(&self, params: Parameters<DepsParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let path = &params.0.path;
        let result = operations::get_deps(&db, path);
        self.file_op_result("deps", path, result)
    }

    // --- Scope --------------------------------------------------------------

    #[tool(
        description = "Show what symbols are visible at a specific line in a file. Lists containing scopes and all symbols defined before that line."
    )]
    // qual:api
    async fn scope(&self, params: Parameters<ScopeParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        let path = &params.0.path;

        match operations::get_scope(&db, path, params.0.line) {
            Ok(result) => {
                let json = savings::record_file_op(&db, "scope", &result, path);
                Ok(Self::success_text(json))
            }
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Files --------------------------------------------------------------

    #[tool(
        description = "List ALL files in the project (indexed + skipped). Unlike overview/search, this shows files with unsupported extensions (.cshtml, .kt, etc.). Use skipped_only=true to find files that need your own tools."
    )]
    // qual:api
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

    // --- Verify -------------------------------------------------------------

    #[tool(
        description = "Verify index integrity. Checks for SQLite corruption, orphan chunks/refs, and files that no longer exist on disk. Use fix=true to auto-repair issues."
    )]
    // qual:api
    async fn verify(&self, params: Parameters<VerifyParams>) -> Result<CallToolResult, McpError> {
        let config = self.config();

        if !config.index_exists() {
            return Ok(Self::error_text(
                "Index not found. Call the 'index' tool first.".into(),
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

    // --- Savings ------------------------------------------------------------

    #[tool(
        description = "Show token savings report: how many tokens rlm saved compared to Claude Code's native tools (Read/Grep/Glob). Optionally filter by date."
    )]
    // qual:api
    async fn savings(&self, params: Parameters<SavingsParams>) -> Result<CallToolResult, McpError> {
        let db = self.ensure_db()?;
        match savings::get_savings_report(&db, params.0.since.as_deref()) {
            Ok(report) => Ok(Self::success_text(Self::to_json(&report))),
            Err(e) => Ok(Self::error_text(e.to_string())),
        }
    }

    // --- Supported ----------------------------------------------------------

    #[tool(
        description = "List all supported file extensions with their language and parser type (tree-sitter, structural, semantic, plaintext)."
    )]
    // qual:api
    async fn supported(&self) -> Result<CallToolResult, McpError> {
        Ok(Self::success_text(Self::to_json(
            &operations::list_supported(),
        )))
    }
}

// -- ServerHandler implementation --------------------------------------------

#[tool_handler]
impl ServerHandler for RlmServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "rlm: Context Broker for semantic code exploration. 18 tools in 4 tiers:\n\
                 ORIENT: overview(detail='minimal'|'standard'|'tree', path?) — project structure at 3 zoom levels.\n\
                 SEARCH: search(query) — full-text across symbols. read(path, symbol|section, metadata?) — symbol body + optional type/signature enrichment.\n\
                 ANALYZE: refs(symbol) — all usages + impact analysis. context(symbol, graph?) — body + callers + callees. deps(path), scope(path, line).\n\
                 EDIT: replace(path, symbol, code, preview?), insert(path, code, position) — Syntax Guard validates all writes.\n\
                 UTILITY: diff, partition, summarize, files, stats, savings, verify, supported, index.\n\
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Search result limit for test queries.
    const TEST_SEARCH_LIMIT: usize = 10;

    /// Setup: create temp dir with test file and index it
    fn setup_indexed_project() -> (TempDir, Config, crate::db::Database) {
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
        let db = crate::db::Database::open(&config.db_path).expect("open db");

        (tmp, config, db)
    }

    #[test]
    fn test_stats_operation_returns_expected_format() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::get_stats(&db).expect("get stats");
        assert!(result.files > 0);
        assert!(result.chunks > 0);
    }

    #[test]
    fn test_search_operation_returns_results() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::search_chunks(&db, "helper", TEST_SEARCH_LIMIT).expect("search");
        assert!(!result.results.is_empty());
    }

    #[test]
    fn test_refs_operation_returns_results() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::analyze_impact(&db, "helper").expect("refs/impact");
        assert!(result.count > 0);
    }

    #[test]
    fn test_context_operation_returns_results() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::build_context(&db, "helper").expect("context");
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("helper"));
    }

    #[test]
    fn test_overview_minimal_operation() {
        use crate::rlm::peek;
        let (_tmp, _config, db) = setup_indexed_project();
        let result = peek::peek(&db, None).expect("peek");
        assert!(!result.files.is_empty());
    }

    #[test]
    fn test_overview_standard_operation() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = operations::build_map(&db, None).expect("map");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_overview_tree_operation() {
        let (_tmp, _config, db) = setup_indexed_project();
        let result = tree::build_tree(&db, None).expect("tree");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_callgraph_in_context_graph() {
        let (_tmp, _config, db) = setup_indexed_project();
        let _ctx = operations::build_context(&db, "helper").expect("context");
        let _graph = operations::build_callgraph(&db, "helper").expect("callgraph");
    }
}
